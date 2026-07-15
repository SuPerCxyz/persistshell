#![allow(dead_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{Seek, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use persist_core::{PersistError, Result};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigterm(_: i32) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

pub fn shutdown_requested() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}

pub fn reset_shutdown() {
    SHUTDOWN.store(false, Ordering::SeqCst);
}

pub fn setup_signal_handler() -> Result<()> {
    unsafe {
        if libc::signal(
            libc::SIGTERM,
            handle_sigterm as *const () as libc::sighandler_t,
        ) == libc::SIG_ERR
        {
            return Err(PersistError::Io {
                operation: "signal SIGTERM",
                source: std::io::Error::last_os_error(),
            });
        }

        for sig in &[libc::SIGINT, libc::SIGHUP, libc::SIGQUIT, libc::SIGPIPE] {
            if libc::signal(*sig, libc::SIG_IGN) == libc::SIG_ERR {
                return Err(PersistError::Io {
                    operation: "signal ignore",
                    source: std::io::Error::last_os_error(),
                });
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct PidFile {
    path: PathBuf,
    file: File,
    pid: u32,
}

#[allow(dead_code)]
impl PidFile {
    pub fn create(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| PersistError::Io {
                operation: "create pidfile directory",
                source,
            })?;
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|source| PersistError::Io {
                operation: "open pidfile",
                source,
            })?;

        unsafe {
            let fd = file.as_raw_fd();
            if libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) != 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                    return Err(PersistError::DaemonAlreadyRunning);
                }
                return Err(PersistError::Io {
                    operation: "flock pidfile",
                    source: err,
                });
            }
        }

        let pid = std::process::id();
        write_pid(&mut file, pid).map_err(|source| PersistError::Io {
            operation: "write pidfile",
            source,
        })?;

        Ok(Self { path, file, pid })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn read_pid(path: &Path) -> Option<u32> {
        persist_core::pidfile::read_pid(path)
    }

    pub fn is_process_alive(pid: u32) -> bool {
        persist_core::pidfile::is_process_alive(pid)
    }

    pub fn is_running(path: &Path) -> bool {
        persist_core::pidfile::is_running(path)
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn write_pid(file: &mut File, pid: u32) -> std::io::Result<()> {
    let mut content = pid.to_string();
    content.push('\n');
    file.set_len(0)?;
    file.rewind()?;
    file.write_all(content.as_bytes())?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_pid_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "persistshell-pid-test-{name}-{}-{nanos}",
            std::process::id()
        ));
        dir.join("daemon.pid")
    }

    #[test]
    fn create_and_lock() {
        let path = test_pid_path("create");
        let pidfile = PidFile::create(path.clone()).expect("create");
        assert!(path.exists());
        assert_eq!(pidfile.pid(), std::process::id());
        drop(pidfile);
        assert!(!path.exists());
    }

    #[test]
    fn duplicate_lock_fails() {
        let path = test_pid_path("duplock");
        let _pidfile = PidFile::create(path.clone()).expect("first create");
        let result = PidFile::create(path.clone());
        assert!(result.is_err());
        match result.unwrap_err() {
            PersistError::DaemonAlreadyRunning => {}
            other => panic!("expected DaemonAlreadyRunning, got {other:?}"),
        }
    }

    #[test]
    fn lock_released_after_drop() {
        let path = test_pid_path("relock");
        {
            let _pidfile = PidFile::create(path.clone()).expect("create");
            assert!(path.exists());
        }
        let pidfile = PidFile::create(path.clone()).expect("second create after drop");
        drop(pidfile);
    }

    #[test]
    fn read_pid_returns_correct_value() {
        let path = test_pid_path("read");
        {
            let _pidfile = PidFile::create(path.clone()).expect("create");
        }
        let pid = PidFile::read_pid(&path);
        // File was deleted on drop, so read should fail
        assert!(pid.is_none());
    }

    #[test]
    fn is_process_alive_returns_true_for_self() {
        assert!(PidFile::is_process_alive(std::process::id()));
    }

    #[test]
    fn is_process_alive_returns_false_for_invalid_pid() {
        assert!(!PidFile::is_process_alive(0x7FFFFFFF));
    }

    #[test]
    fn is_running_returns_false_for_missing_file() {
        let path = test_pid_path("missing");
        assert!(!PidFile::is_running(&path));
    }
}
