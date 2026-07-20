use std::fs;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::lifecycle::{current_uid, io_error};
use persist_core::{PersistError, Result};

const SOCKET_MODE: u32 = 0o600;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct HolderSocket {
    listener: UnixListener,
    path: PathBuf,
}

impl HolderSocket {
    pub(crate) fn bind(path: PathBuf) -> Result<Self> {
        remove_proven_stale_socket(&path)?;
        let listener =
            UnixListener::bind(&path).map_err(|source| io_error("bind holder socket", source))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(SOCKET_MODE))
            .map_err(|source| io_error("set holder socket permissions", source))?;
        validate_socket(&path)?;
        listener
            .set_nonblocking(true)
            .map_err(|source| io_error("set holder listener nonblocking", source))?;
        Ok(Self { listener, path })
    }

    pub(crate) fn accept(&self) -> Result<Option<PeerConnection>> {
        let (stream, _) = match self.listener.accept() {
            Ok(pair) => pair,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(source) => return Err(io_error("accept holder connection", source)),
        };
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|source| io_error("set holder read timeout", source))?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(|source| io_error("set holder write timeout", source))?;
        let credentials = peer_credentials(&stream)?;
        if credentials.uid != current_uid() {
            return Err(PersistError::invalid_argument(format!(
                "holder peer uid {} does not match {}",
                credentials.uid,
                current_uid()
            )));
        }
        Ok(Some(PeerConnection {
            stream,
            pid: credentials.pid,
            uid: credentials.uid,
        }))
    }
}

impl AsRawFd for HolderSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }
}

impl Drop for HolderSocket {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
            metadata.file_type().is_socket() && metadata.uid() == current_uid()
        }) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(crate) struct PeerConnection {
    pub(crate) stream: UnixStream,
    pub(crate) pid: u32,
    pub(crate) uid: u32,
}

impl AsRawFd for PeerConnection {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

impl PeerConnection {
    pub(crate) fn is_closed(&self) -> bool {
        let mut byte = 0u8;
        let result = unsafe {
            libc::recv(
                self.stream.as_raw_fd(),
                (&mut byte as *mut u8).cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        if result == 0 {
            return true;
        }
        if result < 0 {
            return std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock;
        }
        false
    }

    pub(crate) fn closes_within(&self, timeout_ms: i32) -> bool {
        if self.is_closed() {
            return true;
        }
        let mut event = libc::pollfd {
            fd: self.as_raw_fd(),
            events: libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLRDHUP,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut event, 1, timeout_ms) };
        ready > 0 && self.is_closed()
    }
}

fn remove_proven_stale_socket(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(io_error("inspect holder socket", source)),
    };
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_socket()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != SOCKET_MODE
    {
        return Err(PersistError::invalid_argument(format!(
            "unsafe existing holder socket: {}",
            path.display()
        )));
    }
    match UnixStream::connect(path) {
        Ok(_) => Err(PersistError::DaemonAlreadyRunning),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            fs::remove_file(path).map_err(|source| io_error("remove stale holder socket", source))
        }
        Err(source) => Err(io_error("probe existing holder socket", source)),
    }
}

fn validate_socket(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| io_error("inspect bound holder socket", source))?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != SOCKET_MODE
    {
        return Err(PersistError::invalid_argument(format!(
            "unsafe bound holder socket: {}",
            path.display()
        )));
    }
    Ok(())
}

struct PeerCredentials {
    pid: u32,
    uid: u32,
}

fn peer_credentials(stream: &UnixStream) -> Result<PeerCredentials> {
    let mut credentials = unsafe { std::mem::zeroed::<libc::ucred>() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 || length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io_error(
            "read holder peer credentials",
            std::io::Error::last_os_error(),
        ));
    }
    let pid = u32::try_from(credentials.pid)
        .map_err(|_| PersistError::invalid_argument("holder peer pid is invalid"))?;
    Ok(PeerCredentials {
        pid,
        uid: credentials.uid,
    })
}
