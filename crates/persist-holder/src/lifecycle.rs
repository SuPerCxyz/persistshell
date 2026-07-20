use std::fs::{self, File, OpenOptions};
use std::io::{Seek, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use persist_core::{PersistError, Result};

const RUNTIME_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;

pub(crate) fn prepare_runtime_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_directory(path, &metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path)
                .map_err(|source| io_error("create holder runtime directory", source))?;
            fs::set_permissions(path, fs::Permissions::from_mode(RUNTIME_MODE))
                .map_err(|source| io_error("set holder runtime directory permissions", source))?;
            let metadata = fs::symlink_metadata(path)
                .map_err(|source| io_error("inspect holder runtime directory", source))?;
            validate_directory(path, &metadata)
        }
        Err(source) => Err(io_error("inspect holder runtime directory", source)),
    }
}

fn validate_directory(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != RUNTIME_MODE
    {
        return Err(PersistError::invalid_argument(format!(
            "unsafe holder runtime directory: {}",
            path.display()
        )));
    }
    Ok(())
}

pub(crate) struct PidLock {
    path: PathBuf,
    file: File,
}

impl PidLock {
    pub(crate) fn acquire(path: PathBuf) -> Result<Self> {
        validate_existing_private_file(&path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(PRIVATE_FILE_MODE)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|source| io_error("open holder pid file", source))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))
            .map_err(|source| io_error("set holder pid file permissions", source))?;
        let lock_result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if lock_result != 0 {
            let source = std::io::Error::last_os_error();
            return if source.raw_os_error() == Some(libc::EWOULDBLOCK) {
                Err(PersistError::DaemonAlreadyRunning)
            } else {
                Err(io_error("lock holder pid file", source))
            };
        }
        write_pid(&mut file)?;
        Ok(Self { path, file })
    }
}

impl Drop for PidLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn validate_existing_private_file(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(io_error("inspect holder pid file", source)),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != PRIVATE_FILE_MODE
    {
        return Err(PersistError::invalid_argument(format!(
            "unsafe holder pid file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn write_pid(file: &mut File) -> Result<()> {
    file.set_len(0)
        .and_then(|_| file.rewind())
        .and_then(|_| writeln!(file, "{}", std::process::id()))
        .and_then(|_| file.flush())
        .map_err(|source| io_error("write holder pid file", source))
}

pub(crate) struct SignalFd {
    fd: RawFd,
    mask: libc::sigset_t,
}

impl SignalFd {
    pub(crate) fn create() -> Result<Self> {
        let mut mask = unsafe { std::mem::zeroed::<libc::sigset_t>() };
        unsafe {
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, libc::SIGTERM);
            libc::sigaddset(&mut mask, libc::SIGINT);
            libc::sigaddset(&mut mask, libc::SIGCHLD);
        }
        let mask_result =
            unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut()) };
        if mask_result != 0 {
            return Err(io_error(
                "block holder signals",
                std::io::Error::from_raw_os_error(mask_result),
            ));
        }
        unsafe {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            libc::signal(libc::SIGPIPE, libc::SIG_IGN);
            libc::signal(libc::SIGQUIT, libc::SIG_IGN);
        }
        let fd = unsafe { libc::signalfd(-1, &mask, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK) };
        if fd < 0 {
            unsafe {
                libc::pthread_sigmask(libc::SIG_UNBLOCK, &mask, std::ptr::null_mut());
            }
            return Err(io_error(
                "create holder signalfd",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(Self { fd, mask })
    }

    pub(crate) fn consume(&self) -> Result<i32> {
        let mut info = unsafe { std::mem::zeroed::<libc::signalfd_siginfo>() };
        let read = unsafe {
            libc::read(
                self.fd,
                (&mut info as *mut libc::signalfd_siginfo).cast(),
                std::mem::size_of::<libc::signalfd_siginfo>(),
            )
        };
        if read < 0 {
            return Err(io_error(
                "read holder signalfd",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(info.ssi_signo as i32)
    }
}

impl AsRawFd for SignalFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl Drop for SignalFd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
            libc::pthread_sigmask(libc::SIG_UNBLOCK, &self.mask, std::ptr::null_mut());
        }
    }
}

pub(crate) fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

pub(crate) fn io_error(operation: &'static str, source: std::io::Error) -> PersistError {
    PersistError::Io { operation, source }
}
