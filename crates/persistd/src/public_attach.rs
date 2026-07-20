use std::io;
use std::os::fd::{AsRawFd, RawFd};

use persist_core::{PersistError, Result};
use persist_ipc::holder::{
    decode_operation_request, decode_session_exited_event, decode_session_exited_event_v2,
    encode_frame_with_minor, encode_resize_request, encode_signal_request, HolderFrame,
    HolderFrameAccumulator, HolderMessageType, ResizeRequest, SignalRequest, HOLDER_PROTOCOL_MINOR,
};
use persist_ipc::{
    decode_resize, decode_signal, encode_op_resp, encode_session_exited, encode_writer_control,
    FrameAccumulator, MessageType, OpRespPayload, SessionExitedPayload, WriterControlPayload,
};

use crate::holder::{ExitContext, HolderDataConnection};

mod context_timer;
mod pending_writes;
use context_timer::ContextTimer;
use pending_writes::PendingWrites;

pub(crate) struct ProxyOutcome {
    pub(crate) exit_context: Option<ExitContext>,
}

pub(crate) fn run(
    public_fd: RawFd,
    session_id: u32,
    mut holder: HolderDataConnection,
    read_write: bool,
    mut observe_context: Option<&mut dyn FnMut()>,
) -> Result<ProxyOutcome> {
    let holder_fd = holder.as_raw_fd();
    let holder_minor = holder.protocol_minor();
    let mut public_frames = FrameAccumulator::new();
    let mut holder_frames = HolderFrameAccumulator::new();
    let mut to_public = PendingWrites::new();
    let mut to_holder = PendingWrites::new();
    let mut writer = read_write;
    let context_timer = observe_context
        .as_ref()
        .map(|_| ContextTimer::start())
        .transpose()?;
    loop {
        let mut pollfds = [
            pollfd(public_fd, to_public.has_data()),
            pollfd(holder_fd, to_holder.has_data()),
            pollfd(context_timer.as_ref().map_or(-1, ContextTimer::fd), false),
        ];
        let ready = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as _, -1) };
        if ready < 0 {
            let source = io::Error::last_os_error();
            if source.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(io_error("poll public holder proxy", source));
        }
        if pollfds[0].revents & libc::POLLIN != 0
            && !read_public(
                public_fd,
                &mut public_frames,
                &mut to_holder,
                &mut to_public,
                session_id,
                writer,
                holder_minor,
            )?
        {
            return Ok(ProxyOutcome { exit_context: None });
        }
        if pollfds[1].revents & libc::POLLIN != 0 {
            if let Some(outcome) = read_holder(
                holder.stream(),
                &mut holder_frames,
                &mut to_public,
                session_id,
                &mut writer,
                holder_minor,
            )? {
                flush_best_effort(public_fd, &mut to_public);
                return Ok(outcome);
            }
        }
        if pollfds[2].revents & libc::POLLIN != 0 {
            if let Some(timer) = &context_timer {
                timer.consume()?;
            }
            if let Some(observer) = observe_context.as_mut() {
                observer();
            }
        }
        if pollfds[0].revents & libc::POLLOUT != 0 {
            to_public.flush(public_fd)?;
        }
        if pollfds[1].revents & libc::POLLOUT != 0 {
            to_holder.flush(holder_fd)?;
        }
        if has_failure(pollfds[0].revents) || has_failure(pollfds[1].revents) {
            return Ok(ProxyOutcome { exit_context: None });
        }
    }
}

fn read_public(
    fd: RawFd,
    frames: &mut FrameAccumulator,
    to_holder: &mut PendingWrites,
    to_public: &mut PendingWrites,
    session_id: u32,
    writer: bool,
    holder_minor: u16,
) -> Result<bool> {
    let mut buffer = [0u8; 64 * 1024];
    let Some(count) = read_fd(fd, &mut buffer)? else {
        return Ok(false);
    };
    frames.feed(&buffer[..count]);
    while let Some(frame) = frames.try_read()? {
        match frame.msg_type {
            MessageType::Stdin if writer => {
                to_holder.push(holder_bytes(
                    HolderMessageType::Input,
                    frame.payload,
                    holder_minor,
                )?)?;
            }
            MessageType::Resize if writer => {
                if let Some(size) = decode_resize(&frame.payload) {
                    if let Some(payload) = holder_resize_payload(size.rows, size.cols) {
                        to_holder.push(holder_bytes(
                            HolderMessageType::Resize,
                            payload,
                            holder_minor,
                        )?)?;
                    }
                }
            }
            MessageType::Signal if writer => {
                if let Some(signal) = decode_signal(&frame.payload) {
                    to_holder.push(holder_bytes(
                        HolderMessageType::Signal,
                        encode_signal_request(&SignalRequest {
                            signal: signal.signal,
                        }),
                        holder_minor,
                    )?)?;
                    to_public.push(public_bytes(
                        MessageType::SignalResp,
                        &encode_op_resp(&OpRespPayload {
                            ok: true,
                            error_msg: String::new(),
                        }),
                    ))?;
                }
            }
            MessageType::Detach | MessageType::Close => return Ok(false),
            MessageType::Ping => {
                let _ = session_id;
                to_public.push(public_bytes(MessageType::Pong, &[]))?;
            }
            _ => {}
        }
    }
    Ok(true)
}

fn read_holder(
    stream: &mut std::os::unix::net::UnixStream,
    frames: &mut HolderFrameAccumulator,
    output: &mut PendingWrites,
    session_id: u32,
    writer: &mut bool,
    holder_minor: u16,
) -> Result<Option<ProxyOutcome>> {
    let mut buffer = [0u8; 64 * 1024];
    let count = match std::io::Read::read(stream, &mut buffer) {
        Ok(0) => return Ok(Some(ProxyOutcome { exit_context: None })),
        Ok(count) => count,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
        Err(source) => return Err(io_error("read holder data proxy", source)),
    };
    frames
        .feed(&buffer[..count])
        .map_err(|error| protocol_error("accumulate holder data", error))?;
    while let Some(frame) = frames
        .try_read_with_minor(holder_minor)
        .map_err(|error| protocol_error("decode holder data", error))?
    {
        match frame.message_type {
            HolderMessageType::Output => {
                output.push(public_bytes(MessageType::Stdout, &frame.payload))?;
            }
            HolderMessageType::WriteGranted | HolderMessageType::WriteRevoked => {
                let event = decode_operation_request(&frame.payload)
                    .map_err(|error| protocol_error("writer event", error))?;
                if event.session_id != session_id {
                    return Err(PersistError::invalid_argument(
                        "holder writer event session mismatch",
                    ));
                }
                *writer = frame.message_type == HolderMessageType::WriteGranted;
                let kind = if *writer {
                    MessageType::WriteGranted
                } else {
                    MessageType::WriteRevoked
                };
                output.push(public_bytes(
                    kind,
                    &encode_writer_control(&WriterControlPayload { session_id }),
                ))?;
            }
            HolderMessageType::SessionExited => {
                let (event_session, exit_code, cwd, environment) =
                    if holder_minor == HOLDER_PROTOCOL_MINOR {
                        let event = decode_session_exited_event_v2(&frame.payload)
                            .map_err(|error| protocol_error("SessionExited v2", error))?;
                        (
                            event.session_id,
                            event.exit_code,
                            event.cwd,
                            event.environment,
                        )
                    } else {
                        let event = decode_session_exited_event(&frame.payload)
                            .map_err(|error| protocol_error("SessionExited", error))?;
                        (event.session_id, event.exit_code, event.cwd, None)
                    };
                if event_session != session_id {
                    return Err(PersistError::invalid_argument(
                        "holder exit event session mismatch",
                    ));
                }
                output.push(public_bytes(
                    MessageType::SessionExited,
                    &encode_session_exited(&SessionExitedPayload {
                        session_id,
                        exit_code,
                    }),
                ))?;
                return Ok(Some(ProxyOutcome {
                    exit_context: Some(ExitContext {
                        session_id,
                        exit_code,
                        cwd,
                        environment,
                    }),
                }));
            }
            _ => {}
        }
    }
    Ok(None)
}

fn holder_bytes(kind: HolderMessageType, payload: Vec<u8>, minor: u16) -> Result<Vec<u8>> {
    encode_frame_with_minor(
        &HolderFrame {
            message_type: kind,
            flags: 0,
            request_id: 0,
            generation: 0,
            payload,
        },
        minor,
    )
    .map_err(|error| protocol_error("encode holder data", error))
}

fn holder_resize_payload(rows: u16, cols: u16) -> Option<Vec<u8>> {
    (rows > 0 && cols > 0).then(|| encode_resize_request(&ResizeRequest { rows, cols }))
}

fn public_bytes(kind: MessageType, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12 + payload.len());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&(kind as u16).to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn pollfd(fd: RawFd, writable: bool) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN | if writable { libc::POLLOUT } else { 0 },
        revents: 0,
    }
}

fn has_failure(events: i16) -> bool {
    events & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
}

fn read_fd(fd: RawFd, buffer: &mut [u8]) -> Result<Option<usize>> {
    let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
    if count > 0 {
        return Ok(Some(count as usize));
    }
    if count == 0 {
        return Ok(None);
    }
    let source = io::Error::last_os_error();
    if source.kind() == io::ErrorKind::WouldBlock {
        return Ok(Some(0));
    }
    Err(io_error("read public attach proxy", source))
}

fn flush_best_effort(fd: RawFd, queue: &mut PendingWrites) {
    let _ = queue.flush(fd);
}

fn protocol_error(context: &str, error: persist_ipc::holder::HolderProtocolError) -> PersistError {
    PersistError::invalid_argument(format!("{context}: {error:?}"))
}

fn io_error(operation: &'static str, source: io::Error) -> PersistError {
    PersistError::Io { operation, source }
}

#[cfg(test)]
mod tests {
    use super::holder_resize_payload;

    #[test]
    fn zero_public_resize_is_not_forwarded_to_holder() {
        assert!(holder_resize_payload(0, 80).is_none());
        assert!(holder_resize_payload(24, 0).is_none());
        assert_eq!(holder_resize_payload(24, 80).unwrap().len(), 4);
    }
}
