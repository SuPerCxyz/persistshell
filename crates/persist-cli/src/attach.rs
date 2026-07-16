use std::io;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};

use persist_core::{Config, PersistError, Result};
use persist_ipc::{
    decode_attach_resp, decode_new_session_resp, encode_attach, encode_detach, encode_resize,
    read_frame, write_frame, AttachPayload, Frame, FrameAccumulator, MessageType, ResizePayload,
};

use crate::terminal::{NonblockingMode, RawMode};

static RESIZE_PENDING: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigwinch(_: i32) {
    RESIZE_PENDING.store(true, Ordering::SeqCst);
}

pub fn attach(config: &Config, session_id: Option<u32>, readonly: bool) -> Result<()> {
    let mut socket = persist_ipc::ClientSocket::connect(&config.paths.socket_path)?;
    let stream = socket.stream();

    // HELLO
    let uid = unsafe { libc::getuid() };
    let hello_payload = persist_ipc::encode_hello(&persist_ipc::HelloPayload {
        protocol_major: 0,
        protocol_minor: 1,
        uid,
        pid: std::process::id(),
    });
    write_frame(
        stream,
        &Frame {
            msg_type: MessageType::Hello,
            flags: 0,
            request_id: 0,
            payload: hello_payload,
        },
    )?;

    // Wait for HELLO_ACK
    let ack_frame = read_frame(stream)?;
    if ack_frame.msg_type != MessageType::HelloAck {
        return Err(PersistError::invalid_argument("expected HELLO_ACK"));
    }

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
    let attach_payload = encode_attach(&AttachPayload { session_id: sid });
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
    let payload = encode_resize(&ResizePayload {
        rows: ws.ws_row,
        cols: ws.ws_col,
    });
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

fn nix_read(fd: std::os::unix::io::RawFd, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock {
                return Ok(0);
            }
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        return Ok(n as usize);
    }
}
