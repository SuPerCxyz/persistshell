#![allow(dead_code)]

use persist_core::{log_message, LogLevel};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

/// Handle for sending PTY output to a session's async log writer.
#[derive(Clone)]
pub struct SessionLogHandle {
    sender: mpsc::Sender<Vec<u8>>,
}

impl SessionLogHandle {
    pub fn write(&self, data: &[u8]) {
        let _ = self.sender.send(data.to_vec());
    }
}

/// Spawn a background log writer thread for a session.
///
/// The writer receives data via channel and writes to disk immediately.
/// Rotates when `max_file_size` is exceeded, keeping at most `max_files`
/// rotated files. File permissions are 0600.
pub fn spawn_session_logger(
    log_path: PathBuf,
    max_file_size: u64,
    max_files: u32,
    cleanup_done: Arc<Mutex<bool>>,
) -> SessionLogHandle {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    thread::spawn(move || {
        let mut writer = SessionLogFileWriter::open(&log_path);

        while let Ok(data) = rx.recv() {
            if data.is_empty() {
                continue;
            }
            if let Some(ref mut w) = writer {
                w.write_all(&data, max_file_size, max_files);
            }
        }

        *cleanup_done.lock().unwrap() = true;
    });

    SessionLogHandle { sender: tx }
}

struct SessionLogFileWriter {
    file: Option<File>,
    path: PathBuf,
    bytes_written: u64,
}

impl SessionLogFileWriter {
    fn open(path: &Path) -> Option<Self> {
        if let Some(parent) = path.parent() {
            if !parent.exists() && fs::create_dir_all(parent).is_err() {
                return None;
            }
            if set_permissions_0700(parent).is_err() {
                return None;
            }
        }
        let file = open_log_file(path).ok()?;

        let current_len = fs::metadata(path).ok().map(|m| m.len()).unwrap_or(0);
        Some(Self {
            file: Some(file),
            path: path.to_path_buf(),
            bytes_written: current_len,
        })
    }

    fn write_all(&mut self, data: &[u8], max_file_size: u64, max_files: u32) {
        let total = self.bytes_written + data.len() as u64;
        if total > max_file_size {
            self.rotate(max_files);
        }
        if let Some(ref mut file) = self.file {
            if let Err(e) = file.write_all(data) {
                let _ = log_message(
                    LogLevel::Error,
                    "session-log",
                    &format!("write failed: {e}"),
                );
                return;
            }
        }
        self.bytes_written += data.len() as u64;
    }

    fn rotate(&mut self, max_files: u32) {
        self.file = None;

        // Shift .N → .N+1 from max_files-1 down to 1
        for i in (1..max_files).rev() {
            let old = rotate_path(&self.path, i);
            let new = rotate_path(&self.path, i + 1);
            if old.exists() {
                let _ = fs::rename(&old, &new);
            }
        }

        // Remove oldest
        let oldest = rotate_path(&self.path, max_files + 1);
        let _ = fs::remove_file(&oldest);

        // Rename current to .1
        let first = rotate_path(&self.path, 1);
        let _ = fs::rename(&self.path, &first);

        // Reopen fresh
        match open_log_file(&self.path) {
            Ok(f) => {
                self.file = Some(f);
                self.bytes_written = 0;
            }
            Err(e) => {
                let _ = log_message(
                    LogLevel::Error,
                    "session-log",
                    &format!("reopen after rotation failed: {e}"),
                );
            }
        }
    }
}

fn open_log_file(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    set_permissions_0600(path)?;
    Ok(file)
}

fn rotate_path(base: &Path, n: u32) -> PathBuf {
    let ext = format!(".{n}");
    let mut p = base.to_path_buf();
    match p.extension() {
        Some(e) => {
            let mut s = e.to_string_lossy().to_string();
            s.push_str(&ext);
            p.set_extension(&s);
        }
        None => {
            p.set_extension(&ext);
        }
    }
    p
}

fn set_permissions_0600(path: &Path) -> std::io::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)
}

fn set_permissions_0700(path: &Path) -> std::io::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn rotate_path_appends_number() {
        let base = Path::new("/tmp/session.log");
        assert_eq!(rotate_path(base, 1), Path::new("/tmp/session.log.1"));
        assert_eq!(rotate_path(base, 2), Path::new("/tmp/session.log.2"));
    }

    #[test]
    fn set_0600_works() {
        let dir = std::env::temp_dir().join(format!("perm-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create dir");
        let path = dir.join("test.log");
        fs::write(&path, b"hello").expect("write");
        set_permissions_0600(&path).expect("set perms");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = fs::metadata(&path).expect("meta");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_open_enforces_private_parent_and_file_modes() {
        let dir = std::env::temp_dir().join(format!(
            "persist-log-perm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let log_path = dir.join("nested/session.log");
        let writer = SessionLogFileWriter::open(&log_path).expect("open log");
        drop(writer);

        let parent_mode = fs::metadata(log_path.parent().expect("parent"))
            .expect("parent metadata")
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&log_path)
            .expect("file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(parent_mode, 0o700);
        assert_eq!(file_mode, 0o600);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn log_writer_writes_and_rotates() {
        let dir = std::env::temp_dir().join(format!("session-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create dir");

        let log_path = dir.join("test.log");
        let handle = spawn_session_logger(log_path.clone(), 10, 3, Arc::new(Mutex::new(false)));

        handle.write(b"1234567890");
        handle.write(b"ABCDE");

        thread::sleep(Duration::from_millis(100));
        drop(handle);
        thread::sleep(Duration::from_millis(50));

        let content = fs::read_to_string(&log_path).unwrap_or_default();
        // First write filled the file exactly (10 bytes = max_file_size)
        // rotation threshold is > max_file_size, so 10 == 10 → no rotation yet
        // Second write: total = 10 + 5 = 15 > 10 → rotate
        // After rotation: fresh file with "ABCDE"
        assert_eq!(content, "ABCDE");

        let rotated = fs::read_to_string(rotate_path(&log_path, 1)).unwrap_or_default();
        assert_eq!(rotated, "1234567890");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_writer_multiple_rotations() {
        let dir = std::env::temp_dir().join(format!("multi-rot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create dir");

        let log_path = dir.join("multi.log");
        let handle = spawn_session_logger(log_path.clone(), 5, 2, Arc::new(Mutex::new(false)));

        // Write 3 chunks of 5 bytes each
        handle.write(b"AAAAA");
        thread::sleep(Duration::from_millis(50));
        handle.write(b"BBBBB");
        thread::sleep(Duration::from_millis(50));
        handle.write(b"CCCCC");

        thread::sleep(Duration::from_millis(100));
        drop(handle);
        thread::sleep(Duration::from_millis(50));

        // Current should have "CCCCC"
        assert_eq!(fs::read_to_string(&log_path).unwrap_or_default(), "CCCCC");
        // .1 should have "BBBBB"
        assert_eq!(
            fs::read_to_string(rotate_path(&log_path, 1)).unwrap_or_default(),
            "BBBBB"
        );
        // .2 should have "AAAAA" (oldest retained as .2 since max_files=2 but we keep .N for N=max_files)
        // .2 should have "AAAAA" — max_files=2 keeps .1 + .2
        assert_eq!(
            fs::read_to_string(rotate_path(&log_path, 2)).unwrap_or_default(),
            "AAAAA"
        );
        // .3 should not exist
        assert!(!rotate_path(&log_path, 3).exists(), "no .3");

        let _ = fs::remove_dir_all(&dir);
    }
}
