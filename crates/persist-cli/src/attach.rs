use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use persist_core::{Config, PersistError, Result};
use persist_ipc::{
    decode_attach_resp, decode_new_session_resp, encode_attach, encode_attach_with_context,
    encode_detach, encode_resize, read_frame, write_frame, AttachPayload, ConnectionEnvironment,
    Frame, FrameAccumulator, MessageType, ResizePayload, ATTACH_CONTEXT_PROTOCOL_MINOR,
};

use crate::terminal::{NonblockingMode, RawMode};

static RESIZE_PENDING: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigwinch(_: i32) {
    RESIZE_PENDING.store(true, Ordering::SeqCst);
}

pub fn attach(config: &Config, session_id: Option<u32>, readonly: bool) -> Result<()> {
    let mut socket = persist_ipc::ClientSocket::connect(&config.paths.socket_path)?;
    let uid = unsafe { libc::getuid() };
    let ack = socket.send_hello(uid, std::process::id())?;
    let stream = socket.stream();

    let sid = if let Some(sid) = session_id {
        // Attach directly to existing session
        sid
    } else {
        // Send NEW_SESSION
        write_frame(
            stream,
            &Frame {
                msg_type: MessageType::NewSession,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )?;

        let resp = read_frame(stream)?;
        if resp.msg_type != MessageType::NewSessionResp {
            return Err(PersistError::invalid_argument("expected NEW_SESSION_RESP"));
        }
        let session = decode_new_session_resp(&resp.payload)
            .ok_or_else(|| PersistError::invalid_argument("invalid NEW_SESSION_RESP"))?;
        session.session_id
    };

    // Send ATTACH
    let connection_env = connection_environment_from_vars(std::env::vars_os(), uid);
    let attach_payload = encode_attach_for_server(sid, ack.protocol_minor, &connection_env);
    let msg_type = if readonly {
        MessageType::AttachReadOnly
    } else {
        MessageType::Attach
    };
    write_frame(
        stream,
        &Frame {
            msg_type,
            flags: 0,
            request_id: 0,
            payload: attach_payload,
        },
    )?;

    let attach_resp = read_frame(stream)?;
    if attach_resp.msg_type != MessageType::AttachResp {
        return Err(PersistError::invalid_argument("expected ATTACH_RESP"));
    }
    let attach = decode_attach_resp(&attach_resp.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid ATTACH_RESP"))?;
    if !attach.ok {
        return Err(PersistError::invalid_argument(format!(
            "attach failed: {}",
            attach.error_msg
        )));
    }

    // Ignore SIGPIPE so broken socket write doesn't kill us
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    if readonly {
        io_loop_readonly(stream)?;
    } else {
        // Set up SIGWINCH handler for terminal resize
        unsafe {
            libc::signal(
                libc::SIGWINCH,
                handle_sigwinch as *const () as libc::sighandler_t,
            );
        }

        // Enter I/O mode
        let _raw = RawMode::enter()?;
        // Send initial terminal size
        send_resize(stream).ok();
        io_loop(stream)?;
    }

    // Send DETACH — this may fail if the daemon already disconnected (e.g. crash)
    let detach_payload = encode_detach(&persist_ipc::DetachPayload { session_id: sid });
    let detached_ok = write_frame(
        stream,
        &Frame {
            msg_type: MessageType::Detach,
            flags: 0,
            request_id: 0,
            payload: detach_payload,
        },
    )
    .is_ok();

    if detached_ok {
        println!("\r\n[detached]");
    } else {
        eprintln!("\r\n[daemon disconnected — session preserved]");
    }
    Ok(())
}

pub fn benchmark_attach(config: &Config, session_id: u32) -> Result<()> {
    let mut socket = persist_ipc::ClientSocket::connect(&config.paths.socket_path)?;
    let uid = unsafe { libc::getuid() };
    let ack = socket.send_hello(uid, std::process::id())?;
    let connection_env = connection_environment_from_vars(std::env::vars_os(), uid);
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::Attach,
            flags: 0,
            request_id: 1,
            payload: encode_attach_for_server(session_id, ack.protocol_minor, &connection_env),
        },
    )?;
    let response = read_frame(socket.stream())?;
    if response.msg_type != MessageType::AttachResp {
        return Err(PersistError::invalid_argument(
            "expected ATTACH_RESP for benchmark probe",
        ));
    }
    let response = decode_attach_resp(&response.payload)
        .ok_or_else(|| PersistError::invalid_argument("invalid ATTACH_RESP payload"))?;
    if !response.ok {
        return Err(PersistError::invalid_argument(response.error_msg));
    }
    Ok(())
}

fn encode_attach_for_server(
    session_id: u32,
    server_minor: u16,
    context: &ConnectionEnvironment,
) -> Vec<u8> {
    let payload = AttachPayload { session_id };
    if server_minor >= ATTACH_CONTEXT_PROTOCOL_MINOR {
        encode_attach_with_context(&payload, context)
    } else {
        encode_attach(&payload)
    }
}

fn connection_environment_from_vars<I, K, V>(variables: I, uid: u32) -> ConnectionEnvironment
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<OsString>,
    V: Into<OsString>,
{
    let mut accepted = Vec::new();
    for (name, value) in variables {
        let (name, value) = (name.into(), value.into());
        let (Some(name), Some(value)) = (name.to_str(), value.to_str()) else {
            continue;
        };
        if name == "SSH_AUTH_SOCK" && !valid_agent_socket(OsStr::new(value), uid) {
            continue;
        }
        if ConnectionEnvironment::from_pairs([(name, value)]).is_some() {
            accepted.push((name.to_owned(), value.to_owned()));
        }
    }
    ConnectionEnvironment::from_pairs(accepted).unwrap_or_default()
}

fn valid_agent_socket(value: &OsStr, uid: u32) -> bool {
    let path = Path::new(value);
    if !path.is_absolute() {
        return false;
    }
    std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_type().is_socket() && metadata.uid() == uid)
}

fn io_loop(stream: &mut std::os::unix::net::UnixStream) -> Result<()> {
    let socket_fd = stream.as_raw_fd();
    let stdin_fd = libc::STDIN_FILENO;

    let _socket_mode = NonblockingMode::enter(socket_fd)?;
    let _stdin_mode = NonblockingMode::enter(stdin_fd)?;

    let mut accumulator = FrameAccumulator::new();
    let mut buf = vec![0u8; 65536];

    loop {
        let mut pfds = [
            libc::pollfd {
                fd: socket_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stdin_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let ret = unsafe { libc::poll(pfds.as_mut_ptr(), 2, -1) };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                // EINTR — likely from SIGWINCH, check resize after poll
            } else {
                break;
            }
        }

        // Check for pending terminal resize
        if RESIZE_PENDING.swap(false, Ordering::SeqCst) {
            let _ = send_resize(stream);
        }

        // Socket readable
        if pfds[0].revents & libc::POLLIN != 0 {
            match nix_read(socket_fd, &mut buf) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    accumulator.feed(&buf[..n]);
                    loop {
                        match accumulator.try_read() {
                            Ok(Some(frame)) => match frame.msg_type {
                                MessageType::Stdout => {
                                    let stdout_fd = libc::STDOUT_FILENO;
                                    unsafe {
                                        libc::write(
                                            stdout_fd,
                                            frame.payload.as_ptr() as *const libc::c_void,
                                            frame.payload.len(),
                                        );
                                    }
                                }
                                MessageType::SessionExited => {
                                    return Ok(());
                                }
                                MessageType::WriteRequest => {
                                    eprintln!("\r\n[another client requested write access]");
                                }
                                MessageType::WriteGranted => {
                                    eprintln!("\r\n[write access granted]");
                                }
                                MessageType::WriteRevoked => {
                                    eprintln!("\r\n[write access moved to another client]");
                                    return Ok(());
                                }
                                MessageType::Detach | MessageType::Close => {
                                    return Ok(());
                                }
                                _ => {}
                            },
                            Ok(None) => break,
                            Err(_) => return Ok(()),
                        }
                    }
                }
            }
        }

        // Stdin readable
        if pfds[1].revents & libc::POLLIN != 0 {
            let mut stdin_buf = [0u8; 4096];
            match nix_read(stdin_fd, &mut stdin_buf) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Ok(0) => break,
                Ok(n) => {
                    let frame = Frame {
                        msg_type: MessageType::Stdin,
                        flags: 0,
                        request_id: 0,
                        payload: stdin_buf[..n].to_vec(),
                    };
                    if write_frame(stream, &frame).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        if pfds[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            break;
        }
    }

    Ok(())
}

fn io_loop_readonly(stream: &mut std::os::unix::net::UnixStream) -> Result<()> {
    let socket_fd = stream.as_raw_fd();
    let _socket_mode = NonblockingMode::enter(socket_fd)?;

    let mut accumulator = FrameAccumulator::new();
    let mut buf = [0u8; 65536];

    loop {
        let mut pfd = [libc::pollfd {
            fd: socket_fd,
            events: libc::POLLIN,
            revents: 0,
        }];

        let ret = unsafe { libc::poll(pfd.as_mut_ptr(), 1, -1) };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::Interrupted {
                break;
            }
            continue;
        }

        if pfd[0].revents & libc::POLLIN != 0 {
            match nix_read(socket_fd, &mut buf) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    accumulator.feed(&buf[..n]);
                    loop {
                        match accumulator.try_read() {
                            Ok(Some(frame)) => match frame.msg_type {
                                MessageType::Stdout => {
                                    let stdout_fd = libc::STDOUT_FILENO;
                                    unsafe {
                                        libc::write(
                                            stdout_fd,
                                            frame.payload.as_ptr() as *const libc::c_void,
                                            frame.payload.len(),
                                        );
                                    }
                                }
                                MessageType::SessionExited => {
                                    return Ok(());
                                }
                                MessageType::Detach | MessageType::Close => {
                                    return Ok(());
                                }
                                _ => {}
                            },
                            Ok(None) => break,
                            Err(_) => return Ok(()),
                        }
                    }
                }
            }
        }

        if pfd[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            break;
        }
    }

    Ok(())
}

fn send_resize(stream: &mut std::os::unix::net::UnixStream) -> Result<()> {
    let mut ws = std::mem::MaybeUninit::<libc::winsize>::uninit();
    let ret = unsafe { libc::ioctl(libc::STDIN_FILENO, libc::TIOCGWINSZ, ws.as_mut_ptr()) };
    if ret < 0 {
        return Ok(());
    }
    let ws = unsafe { ws.assume_init() };
    let Some(payload) = terminal_resize_payload(ws.ws_row, ws.ws_col) else {
        return Ok(());
    };
    write_frame(
        stream,
        &Frame {
            msg_type: MessageType::Resize,
            flags: 0,
            request_id: 0,
            payload,
        },
    )?;
    Ok(())
}

fn terminal_resize_payload(rows: u16, cols: u16) -> Option<Vec<u8>> {
    (rows > 0 && cols > 0).then(|| encode_resize(&ResizePayload { rows, cols }))
}

fn nix_read(fd: std::os::unix::io::RawFd, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        return Ok(n as usize);
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;

    use persist_ipc::{decode_attach, ATTACH_CONTEXT_PROTOCOL_MINOR};

    use super::{
        connection_environment_from_vars, encode_attach_for_server, nix_read,
        terminal_resize_payload,
    };

    #[test]
    fn zero_terminal_size_is_not_forwarded() {
        assert!(terminal_resize_payload(0, 80).is_none());
        assert!(terminal_resize_payload(24, 0).is_none());
        assert!(terminal_resize_payload(24, 80).is_some());
    }

    #[test]
    fn nonblocking_read_keeps_would_block_distinct_from_eof() {
        let mut fds = [0; 2];
        assert_eq!(
            unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK) },
            0
        );
        let mut buffer = [0; 1];

        let error = nix_read(fds[0], &mut buffer).expect_err("empty pipe must block");
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);

        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
    }

    #[test]
    fn connection_environment_uses_fixed_allowlist_and_valid_agent_socket() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join("agent.sock");
        let _listener = UnixListener::bind(&socket).expect("bind agent");
        let variables = vec![
            (OsString::from("TERM"), OsString::from("xterm-256color")),
            (
                OsString::from("SSH_AUTH_SOCK"),
                socket.as_os_str().to_owned(),
            ),
            (OsString::from("API_TOKEN"), OsString::from("secret")),
            (OsString::from("DISPLAY"), OsString::from("bad\nvalue")),
            (OsString::from("COLORTERM"), OsString::from_vec(vec![0xff])),
        ];

        let context = connection_environment_from_vars(variables, unsafe { libc::getuid() });
        let collected: Vec<_> = context.iter().collect();
        assert!(collected.contains(&("TERM", "xterm-256color")));
        assert!(collected.contains(&("SSH_AUTH_SOCK", socket.to_str().expect("utf8 path"))));
        assert_eq!(collected.len(), 2);
    }

    #[test]
    fn connection_environment_rejects_non_socket_and_symlink_agent_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = temp.path().join("agent.file");
        fs::write(&file, b"not a socket").expect("write");
        let socket = temp.path().join("agent.sock");
        let _listener = UnixListener::bind(&socket).expect("bind");
        let link = temp.path().join("agent.link");
        symlink(&socket, &link).expect("symlink");

        for path in [file, link] {
            let context = connection_environment_from_vars(
                [(OsString::from("SSH_AUTH_SOCK"), path.into_os_string())],
                unsafe { libc::getuid() },
            );
            assert!(context.is_empty());
        }
    }

    #[test]
    fn attach_encoding_respects_server_minor() {
        let context =
            persist_ipc::ConnectionEnvironment::from_pairs([("TERM", "screen")]).expect("context");

        let legacy = encode_attach_for_server(9, 1, &context);
        assert_eq!(legacy, 9_u32.to_be_bytes());

        let current = encode_attach_for_server(9, ATTACH_CONTEXT_PROTOCOL_MINOR, &context);
        let decoded = decode_attach(&current).expect("decode");
        assert_eq!(decoded.connection_env, context);
    }
}
