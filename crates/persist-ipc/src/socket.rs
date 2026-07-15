use std::fs;
use std::mem;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use libc::{c_void, getsockopt, socklen_t, SO_PEERCRED};
use persist_core::{PersistError, Result};

use crate::protocol::{
    decode_hello, decode_hello_ack, encode_hello, encode_hello_ack, read_frame, set_stream_timeout,
    write_frame, Frame, HelloAckPayload, HelloPayload, HelloStatus, MessageType, ProtocolVersion,
};

const SOCKET_DIR_MODE: u32 = 0o700;
const SOCKET_FILE_MODE: u32 = 0o600;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct DaemonSocket {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl DaemonSocket {
    pub fn bind(socket_path: PathBuf) -> Result<Self> {
        let parent = socket_path.parent().ok_or_else(|| {
            PersistError::invalid_argument(format!(
                "socket path has no parent: {}",
                socket_path.display()
            ))
        })?;

        fs::create_dir_all(parent).map_err(|source| PersistError::Io {
            operation: "create socket directory",
            source,
        })?;

        set_dir_permission(parent, SOCKET_DIR_MODE)?;

        if socket_path.exists() {
            remove_socket_file(&socket_path, "remove stale socket")?;
        }

        let listener = UnixListener::bind(&socket_path).map_err(|source| PersistError::Io {
            operation: "bind socket",
            source,
        })?;

        set_file_permission(&socket_path, SOCKET_FILE_MODE)?;

        Ok(Self {
            listener,
            socket_path,
        })
    }

    pub fn accept(&self) -> Result<DaemonConnection> {
        let (stream, _addr) = self.listener.accept().map_err(|source| PersistError::Io {
            operation: "accept connection",
            source,
        })?;

        self.verify_peer(stream)
    }

    pub fn accept_timeout(&self, timeout: Duration) -> Result<Option<DaemonConnection>> {
        let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        let mut pfd = libc::pollfd {
            fd: self.listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if ready < 0 {
            let source = std::io::Error::last_os_error();
            if source.kind() == std::io::ErrorKind::Interrupted {
                return Ok(None);
            }
            return Err(PersistError::Io {
                operation: "wait for socket connection",
                source,
            });
        }
        if ready == 0 {
            return Ok(None);
        }

        let (stream, _addr) = self.listener.accept().map_err(|source| PersistError::Io {
            operation: "accept connection",
            source,
        })?;

        self.verify_peer(stream).map(Some)
    }

    fn verify_peer(&self, stream: UnixStream) -> Result<DaemonConnection> {
        let peer_uid = get_peer_uid(&stream)?;
        let our_uid = current_uid();

        if peer_uid != our_uid {
            return Err(PersistError::invalid_argument(format!(
                "connection rejected: peer uid {peer_uid} != daemon uid {our_uid}"
            )));
        }

        Ok(DaemonConnection { stream })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn cleanup(&self) -> Result<()> {
        if self.socket_path.exists() {
            remove_socket_file(&self.socket_path, "cleanup socket file")
        } else {
            Ok(())
        }
    }

    pub fn receive_hello(&self, conn: &mut DaemonConnection) -> Result<HelloPayload> {
        set_stream_timeout(&conn.stream, Some(DEFAULT_HANDSHAKE_TIMEOUT), None)?;
        let frame = read_frame(&mut conn.stream)?;

        if frame.msg_type != MessageType::Hello {
            return Err(PersistError::invalid_argument(format!(
                "expected HELLO, got {:?}",
                frame.msg_type
            )));
        }

        let hello = decode_hello(&frame.payload)
            .ok_or_else(|| PersistError::invalid_argument("malformed HELLO payload"))?;

        Ok(hello)
    }

    pub fn send_ack(&self, conn: &mut DaemonConnection, status: HelloStatus) -> Result<()> {
        let server_pid = std::process::id();
        let ack = HelloAckPayload {
            protocol_major: ProtocolVersion::CURRENT.major,
            protocol_minor: ProtocolVersion::CURRENT.minor,
            pid: server_pid,
            status,
        };

        let payload = encode_hello_ack(&ack);
        set_stream_timeout(&conn.stream, None, Some(DEFAULT_HANDSHAKE_TIMEOUT))?;
        write_frame(
            &mut conn.stream,
            &Frame {
                msg_type: MessageType::HelloAck,
                flags: 0,
                request_id: 0,
                payload,
            },
        )
    }
}

impl Drop for DaemonSocket {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[derive(Debug)]
pub struct DaemonConnection {
    stream: UnixStream,
}

impl DaemonConnection {
    pub fn stream(&mut self) -> &mut UnixStream {
        &mut self.stream
    }
}

#[derive(Debug)]
pub struct ClientSocket {
    stream: UnixStream,
}

impl ClientSocket {
    pub fn connect(socket_path: &Path) -> Result<Self> {
        let stream = connect_with_timeout(socket_path, DEFAULT_CONNECT_TIMEOUT)?;

        set_stream_timeout(
            &stream,
            Some(DEFAULT_HANDSHAKE_TIMEOUT),
            Some(DEFAULT_HANDSHAKE_TIMEOUT),
        )?;

        Ok(Self { stream })
    }

    pub fn send_hello(&mut self, uid: u32, pid: u32) -> Result<HelloAckPayload> {
        let hello = HelloPayload {
            protocol_major: ProtocolVersion::CURRENT.major,
            protocol_minor: ProtocolVersion::CURRENT.minor,
            uid,
            pid,
        };

        let payload = encode_hello(&hello);
        write_frame(
            &mut self.stream,
            &Frame {
                msg_type: MessageType::Hello,
                flags: 0,
                request_id: 0,
                payload,
            },
        )?;

        let frame = read_frame(&mut self.stream)?;

        if frame.msg_type != MessageType::HelloAck {
            return Err(PersistError::invalid_argument(format!(
                "expected HELLO_ACK, got {:?}",
                frame.msg_type
            )));
        }

        let ack = decode_hello_ack(&frame.payload)
            .ok_or_else(|| PersistError::invalid_argument("malformed HELLO_ACK payload"))?;
        self.clear_timeouts()?;
        Ok(ack)
    }

    /// Clears handshake-only socket timeouts before processing a long operation.
    pub fn clear_timeouts(&self) -> Result<()> {
        set_stream_timeout(&self.stream, None, None)
    }

    pub fn stream(&mut self) -> &mut UnixStream {
        &mut self.stream
    }
}

pub fn check_socket_path(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if parent.exists() {
            let metadata = fs::metadata(parent).map_err(|source| PersistError::Io {
                operation: "check socket directory metadata",
                source,
            })?;
            let permissions = metadata.permissions();
            if permissions.mode() & 0o777 != SOCKET_DIR_MODE {
                return Err(PersistError::invalid_argument(format!(
                    "socket directory has insecure permissions: {:o}, expected {:o}",
                    permissions.mode() & 0o777,
                    SOCKET_DIR_MODE
                )));
            }
        }
    }

    if path.exists() {
        let metadata = fs::metadata(path).map_err(|source| PersistError::Io {
            operation: "check socket file metadata",
            source,
        })?;
        let permissions = metadata.permissions();
        if permissions.mode() & 0o777 != SOCKET_FILE_MODE {
            return Err(PersistError::invalid_argument(format!(
                "socket file has insecure permissions: {:o}, expected {:o}",
                permissions.mode() & 0o777,
                SOCKET_FILE_MODE
            )));
        }
    }

    Ok(())
}

pub fn cleanup_stale_socket(path: &Path) -> Result<()> {
    if path.exists() {
        remove_socket_file(path, "remove stale socket file")
    } else {
        Ok(())
    }
}

fn remove_socket_file(path: &Path, operation: &'static str) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| PersistError::Io { operation, source })?;
    if !metadata.file_type().is_socket() {
        return Err(PersistError::invalid_argument(format!(
            "refusing to remove non-socket path: {}",
            path.display()
        )));
    }
    fs::remove_file(path).map_err(|source| PersistError::Io { operation, source })
}

fn connect_with_timeout(path: &Path, timeout: Duration) -> Result<UnixStream> {
    let socket_path = path.to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result = UnixStream::connect(&socket_path);
        let _ = sender.send(result);
    });

    match receiver.recv_timeout(timeout) {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(source)) => Err(PersistError::Io {
            operation: "connect socket",
            source,
        }),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(PersistError::invalid_argument(format!(
                "connection to {} timed out after {}ms",
                path.display(),
                timeout.as_millis()
            )))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(PersistError::Internal {
            message: "connection thread disconnected unexpectedly".to_string(),
        }),
    }
}

fn set_dir_permission(path: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|source| PersistError::Io {
        operation: "set socket directory permission",
        source,
    })
}

fn get_peer_uid(stream: &UnixStream) -> Result<u32> {
    let fd = stream.as_raw_fd();
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = mem::size_of::<libc::ucred>() as socklen_t;

    let result = unsafe {
        getsockopt(
            fd,
            libc::SOL_SOCKET,
            SO_PEERCRED,
            &mut cred as *mut _ as *mut c_void,
            &mut len,
        )
    };

    if result != 0 {
        return Err(PersistError::Io {
            operation: "getsockopt SO_PEERCRED",
            source: std::io::Error::last_os_error(),
        });
    }

    Ok(cred.uid)
}

fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

fn set_file_permission(path: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|source| PersistError::Io {
        operation: "set socket file permission",
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "persistshell-ipc-test-{name}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test dir");
            Self { path }
        }

        fn socket_path(&self) -> PathBuf {
            self.path.join("test.sock")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn daemon_binds_and_accepts() {
        let dir = TestDir::new("bind");
        let socket_path = dir.socket_path();

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        assert!(socket_path.exists());

        let child_path = socket_path.clone();
        let handle = std::thread::spawn(move || {
            let mut client = ClientSocket::connect(&child_path).expect("connect");
            client.send_hello(0, 0).expect("send hello");
        });

        let mut conn = daemon.accept().expect("accept");
        let hello = daemon.receive_hello(&mut conn).expect("receive hello");
        assert_eq!(hello.protocol_major, ProtocolVersion::CURRENT.major);
        daemon
            .send_ack(&mut conn, HelloStatus::Accepted)
            .expect("send ack");

        handle.join().expect("join");
        drop(daemon);
        assert!(!socket_path.exists());
    }

    #[test]
    fn socket_permissions_are_correct() {
        let dir = TestDir::new("perms");
        let socket_path = dir.socket_path();

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");

        let dir_meta = fs::metadata(&dir.path).expect("dir meta");
        assert_eq!(
            dir_meta.permissions().mode() & 0o777,
            SOCKET_DIR_MODE,
            "dir should be 0700"
        );

        let sock_meta = fs::metadata(&socket_path).expect("sock meta");
        assert_eq!(
            sock_meta.permissions().mode() & 0o777,
            SOCKET_FILE_MODE,
            "socket should be 0600"
        );

        drop(daemon);
    }

    #[test]
    fn cleanup_removes_socket_file() {
        let dir = TestDir::new("cleanup");
        let socket_path = dir.socket_path();

        {
            let _daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
            assert!(socket_path.exists());
        }

        assert!(!socket_path.exists(), "socket should be cleaned up on drop");
    }

    #[test]
    fn cleanup_stale_rejects_regular_file() {
        let dir = TestDir::new("stale");
        let socket_path = dir.socket_path();
        fs::write(&socket_path, b"stale").expect("write stale socket");

        assert!(socket_path.exists());
        assert!(cleanup_stale_socket(&socket_path).is_err());
        assert!(socket_path.exists());
    }

    #[test]
    fn hello_handshake_completes() {
        let dir = TestDir::new("handshake");
        let socket_path = dir.socket_path();

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");

        let child_path = socket_path.clone();
        let handle = std::thread::spawn(move || {
            let mut client = ClientSocket::connect(&child_path).expect("connect");
            let ack = client.send_hello(1000, 42).expect("send hello");
            assert_eq!(ack.protocol_major, ProtocolVersion::CURRENT.major);
            assert_eq!(ack.status, HelloStatus::Accepted);
        });

        let mut conn = daemon.accept().expect("accept");
        let hello = daemon.receive_hello(&mut conn).expect("receive hello");
        assert_eq!(hello.uid, 1000);

        daemon
            .send_ack(&mut conn, HelloStatus::Accepted)
            .expect("send ack");

        handle.join().expect("join");
        drop(daemon);
    }

    #[test]
    fn clear_timeouts_removes_handshake_limit() {
        let (stream, _peer) = UnixStream::pair().expect("pair");
        set_stream_timeout(
            &stream,
            Some(Duration::from_secs(5)),
            Some(Duration::from_secs(5)),
        )
        .expect("set handshake timeout");
        let client = ClientSocket { stream };

        client.clear_timeouts().expect("clear timeouts");
        assert_eq!(client.stream.read_timeout().expect("read timeout"), None);
        assert_eq!(client.stream.write_timeout().expect("write timeout"), None);
    }

    #[test]
    fn socket_dir_created_automatically() {
        let dir = TestDir::new("auto-dir");
        let nested = dir.path.join("nested").join("subdir");
        let socket_path = nested.join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        assert!(nested.exists());
        assert!(socket_path.exists());
        drop(daemon);
    }
}
