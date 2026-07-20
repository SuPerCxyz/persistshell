use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use persist_core::{PersistError, Result};

const MAX_LOG_BACKLOG: usize = 8 * 1024 * 1024;
const LOG_MODE: u32 = 0o600;

struct LogItem {
    session_id: u32,
    path: PathBuf,
    data: Vec<u8>,
}

#[derive(Default)]
struct QueueState {
    items: VecDeque<LogItem>,
    bytes: usize,
    stopping: bool,
}

struct Shared {
    queue: Mutex<QueueState>,
    ready: Condvar,
    failures: Mutex<VecDeque<(u32, u64)>>,
    event_fd: RawFd,
    max_file_size: u64,
    max_files: u32,
}

pub(crate) struct LogWorker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl LogWorker {
    pub(crate) fn start(max_file_size: u64, max_files: u32) -> Result<Self> {
        let event_fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if event_fd < 0 {
            return Err(io_error("create holder log eventfd"));
        }
        let shared = Arc::new(Shared {
            queue: Mutex::new(QueueState::default()),
            ready: Condvar::new(),
            failures: Mutex::new(VecDeque::new()),
            event_fd,
            max_file_size,
            max_files,
        });
        let worker_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("persist-holder-log".into())
            .spawn(move || worker_loop(worker_shared))
            .map_err(|source| PersistError::Io {
                operation: "start holder log worker",
                source,
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub(crate) fn event_fd(&self) -> RawFd {
        self.shared.event_fd
    }

    pub(crate) fn enqueue(&self, session_id: u32, path: &Path, data: &[u8]) -> u64 {
        let mut queue = self.shared.queue.lock().unwrap();
        let Some(new_size) = queue.bytes.checked_add(data.len()) else {
            return data.len() as u64;
        };
        if new_size > MAX_LOG_BACKLOG {
            return data.len() as u64;
        }
        queue.bytes = new_size;
        queue.items.push_back(LogItem {
            session_id,
            path: path.to_path_buf(),
            data: data.to_vec(),
        });
        self.shared.ready.notify_one();
        0
    }

    pub(crate) fn take_failures(&self) -> Vec<(u32, u64)> {
        let mut counter = 0u64;
        unsafe {
            libc::read(
                self.shared.event_fd,
                (&mut counter as *mut u64).cast(),
                std::mem::size_of::<u64>(),
            );
        }
        self.shared.failures.lock().unwrap().drain(..).collect()
    }
}

impl AsRawFd for LogWorker {
    fn as_raw_fd(&self) -> RawFd {
        self.event_fd()
    }
}

impl Drop for LogWorker {
    fn drop(&mut self) {
        {
            let mut queue = self.shared.queue.lock().unwrap();
            queue.stopping = true;
            self.shared.ready.notify_one();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        unsafe { libc::close(self.shared.event_fd) };
    }
}

fn worker_loop(shared: Arc<Shared>) {
    let mut files = HashMap::<PathBuf, LogFile>::new();
    loop {
        let item = {
            let mut queue = shared.queue.lock().unwrap();
            while queue.items.is_empty() && !queue.stopping {
                queue = shared.ready.wait(queue).unwrap();
            }
            if queue.items.is_empty() && queue.stopping {
                return;
            }
            let item = queue.items.pop_front().unwrap();
            queue.bytes -= item.data.len();
            item
        };
        let result = write_item(&mut files, &item, shared.max_file_size, shared.max_files);
        if result.is_err() {
            files.remove(&item.path);
            shared
                .failures
                .lock()
                .unwrap()
                .push_back((item.session_id, item.data.len() as u64));
            let one = 1u64;
            unsafe {
                libc::write(
                    shared.event_fd,
                    (&one as *const u64).cast(),
                    std::mem::size_of::<u64>(),
                );
            }
        }
    }
}

struct LogFile {
    file: File,
    bytes_written: u64,
}

fn write_item(
    files: &mut HashMap<PathBuf, LogFile>,
    item: &LogItem,
    max_file_size: u64,
    max_files: u32,
) -> std::io::Result<()> {
    if !files.contains_key(&item.path) {
        files.insert(item.path.clone(), open_log(&item.path)?);
    }
    let log = files.get_mut(&item.path).unwrap();
    if log.bytes_written.saturating_add(item.data.len() as u64) > max_file_size {
        rotate_log(log, &item.path, max_files)?;
    }
    log.file.write_all(&item.data)?;
    log.bytes_written = log.bytes_written.saturating_add(item.data.len() as u64);
    Ok(())
}

fn open_log(path: &Path) -> std::io::Result<LogFile> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(LOG_MODE)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::getuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "unsafe holder session log",
        ));
    }
    Ok(LogFile {
        bytes_written: metadata.len(),
        file,
    })
}

fn rotate_log(log: &mut LogFile, path: &Path, max_files: u32) -> std::io::Result<()> {
    let _ = std::fs::remove_file(rotated_path(path, max_files));
    for number in (1..max_files).rev() {
        let old = rotated_path(path, number);
        if old.exists() {
            std::fs::rename(old, rotated_path(path, number + 1))?;
        }
    }
    if max_files > 0 {
        std::fs::rename(path, rotated_path(path, 1))?;
    } else {
        std::fs::remove_file(path)?;
    }
    *log = open_log(path)?;
    Ok(())
}

fn rotated_path(path: &Path, number: u32) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), number))
}

fn io_error(operation: &'static str) -> PersistError {
    PersistError::Io {
        operation,
        source: std::io::Error::last_os_error(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_rotates_with_bounded_backlog() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.log");
        let worker = LogWorker::start(5, 2).unwrap();
        assert_eq!(worker.enqueue(1, &path, b"aaaaa"), 0);
        assert_eq!(worker.enqueue(1, &path, b"bbbbb"), 0);
        assert_eq!(worker.enqueue(1, &path, b"ccccc"), 0);
        drop(worker);
        assert_eq!(std::fs::read(&path).unwrap(), b"ccccc");
        assert_eq!(std::fs::read(rotated_path(&path, 1)).unwrap(), b"bbbbb");
        assert_eq!(std::fs::read(rotated_path(&path, 2)).unwrap(), b"aaaaa");
        assert!(!rotated_path(&path, 3).exists());
    }
}
