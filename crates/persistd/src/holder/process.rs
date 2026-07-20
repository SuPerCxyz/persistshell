use std::ffi::CString;
use std::fs;
use std::os::fd::RawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use persist_core::{PersistError, Result};

use super::client::HolderControlClient;
use super::process_watch::{ProcessExit, TimerFd, FALLBACK_TICK};

const INSTALLED_HOLDER: &str = "/usr/libexec/persistshell/persist-holder";
const START_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn resolve_holder_binary() -> Result<Option<PathBuf>> {
    let (configured, sibling) = if cfg!(debug_assertions) {
        (
            std::env::var_os("PERSIST_HOLDER_PATH").map(PathBuf::from),
            development_sibling(),
        )
    } else {
        (None, None)
    };
    let Some(path) = select_holder_binary(cfg!(debug_assertions), configured, sibling) else {
        return Ok(None);
    };
    validate_binary(&path)?;
    Ok(Some(path))
}

pub(super) fn select_holder_binary(
    development_build: bool,
    configured: Option<PathBuf>,
    sibling: Option<PathBuf>,
) -> Option<PathBuf> {
    if development_build {
        configured.or(sibling)
    } else {
        Some(PathBuf::from(INSTALLED_HOLDER))
    }
}

pub(crate) fn connect_or_start(
    runtime_dir: &Path,
    socket_path: &Path,
    binary: &Path,
) -> Result<(HolderControlClient, Option<Child>)> {
    match HolderControlClient::connect(socket_path) {
        Ok(client) => return Ok((client, None)),
        Err(error) if UnixStream::connect(socket_path).is_ok() => return Err(error),
        Err(_) => {}
    }
    validate_binary(binary)?;
    let watcher = StartupWatcher::new(runtime_dir)?;
    let holder_stderr =
        if cfg!(debug_assertions) && std::env::var_os("PERSIST_TEST_CRASH_POINT").is_some() {
            Stdio::inherit()
        } else {
            Stdio::null()
        };
    let mut child = Command::new(binary)
        .arg("foreground")
        .env_remove("PERSIST_TEST_CRASH_POINT")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(holder_stderr)
        .spawn()
        .map_err(|source| io_error("start persist-holder", source))?;
    let process_exit = ProcessExit::watch(child.id())?;
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(client) = HolderControlClient::connect(socket_path) {
            return Ok((client, Some(child)));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|source| io_error("inspect persist-holder startup", source))?
        {
            return Err(PersistError::internal_error(format!(
                "persist-holder exited during startup: {status}"
            )));
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(PersistError::internal_error(
                "timed out waiting for persist-holder socket",
            ));
        }
        watcher.wait(&process_exit, deadline.saturating_duration_since(now))?;
    }
}

pub(super) fn validate_binary(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(PersistError::invalid_argument(
            "persist-holder binary path must be absolute",
        ));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect persist-holder binary", source))?;
    let uid = unsafe { libc::getuid() };
    let owner = metadata.uid();
    let mode = metadata.permissions().mode();
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || (owner != 0 && owner != uid)
        || mode & 0o111 == 0
        || mode & 0o022 != 0
    {
        return Err(PersistError::invalid_argument(
            "persist-holder binary is not a trusted executable",
        ));
    }
    Ok(())
}

fn development_sibling() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let parent = executable.parent()?;
    for directory in [Some(parent), parent.parent()].into_iter().flatten() {
        let candidate = directory.join("persist-holder");
        if candidate.exists() && validate_binary(&candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

struct StartupWatcher {
    fd: RawFd,
}

impl StartupWatcher {
    fn new(runtime_dir: &Path) -> Result<Self> {
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if fd < 0 {
            return Err(io_error(
                "create holder startup watcher",
                std::io::Error::last_os_error(),
            ));
        }
        let path = CString::new(runtime_dir.as_os_str().as_encoded_bytes())
            .map_err(|_| PersistError::invalid_argument("runtime path contains null byte"))?;
        let watch = unsafe {
            libc::inotify_add_watch(
                fd,
                path.as_ptr(),
                libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_ATTRIB,
            )
        };
        if watch < 0 {
            unsafe { libc::close(fd) };
            return Err(io_error(
                "watch holder runtime directory",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(Self { fd })
    }

    fn wait(&self, process_exit: &ProcessExit, timeout: Duration) -> Result<()> {
        let timer = if process_exit.poll_fd().is_none() {
            Some(TimerFd::one_shot(timeout.min(FALLBACK_TICK))?)
        } else {
            None
        };
        let mut fds = vec![libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        }];
        if let Some(fd) = process_exit
            .poll_fd()
            .or_else(|| timer.as_ref().map(TimerFd::fd))
        {
            fds.push(libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }
        let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, millis) };
        if ready < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(io_error(
                "wait for persist-holder startup",
                std::io::Error::last_os_error(),
            ));
        }
        if fds[0].revents & libc::POLLIN != 0 {
            let mut buffer = [0u8; 4096];
            unsafe { libc::read(self.fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        }
        Ok(())
    }
}

impl Drop for StartupWatcher {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

fn io_error(operation: &'static str, source: std::io::Error) -> PersistError {
    PersistError::Io { operation, source }
}
