use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use persist_core::{PersistError, Result};
use persist_ipc::holder::*;

pub(crate) struct HolderDataConnection {
    stream: UnixStream,
    protocol_minor: u16,
}

impl HolderDataConnection {
    pub(crate) fn connect(
        path: &Path,
        instance_id: [u8; 16],
        nonce: [u8; 16],
        protocol_minor: u16,
        session_id: u32,
        mode: HolderAttachMode,
        replay_bytes: u32,
    ) -> Result<Self> {
        let mut stream = UnixStream::connect(path)
            .map_err(|source| io_error("connect holder data socket", source))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .and_then(|_| stream.set_write_timeout(Some(Duration::from_secs(2))))
            .map_err(|source| io_error("configure holder data timeout", source))?;
        exchange(
            &mut stream,
            HolderFrame {
                message_type: HolderMessageType::DataHello,
                flags: 0,
                request_id: 1,
                generation: 0,
                payload: encode_data_hello(&DataHello {
                    daemon_pid: std::process::id(),
                    instance_id,
                    nonce,
                }),
            },
            HolderMessageType::DataHelloAck,
            protocol_minor,
            |payload| {
                let ack = decode_data_hello_ack(payload)
                    .map_err(|error| protocol_error("DataHelloAck", error))?;
                if ack.instance_id != instance_id
                    || ack.nonce != nonce
                    || ack.status != HelloStatus::Accepted
                {
                    return Err(PersistError::invalid_argument(
                        "holder rejected data connection",
                    ));
                }
                Ok(())
            },
        )?;
        exchange(
            &mut stream,
            HolderFrame {
                message_type: HolderMessageType::Attach,
                flags: 0,
                request_id: 2,
                generation: 0,
                payload: encode_attach_request(&AttachRequest {
                    session_id,
                    mode,
                    replay_bytes,
                }),
            },
            HolderMessageType::AttachResp,
            protocol_minor,
            |payload| {
                let response = decode_operation_response(payload)
                    .map_err(|error| protocol_error("AttachResp", error))?;
                if response.status != OperationStatus::Ok {
                    return Err(PersistError::invalid_argument(format!(
                        "holder attach failed: {}",
                        response.message
                    )));
                }
                Ok(())
            },
        )?;
        stream
            .set_read_timeout(None)
            .and_then(|_| stream.set_write_timeout(None))
            .and_then(|_| stream.set_nonblocking(true))
            .map_err(|source| io_error("set holder data nonblocking", source))?;
        Ok(Self {
            stream,
            protocol_minor,
        })
    }

    pub(crate) fn stream(&mut self) -> &mut UnixStream {
        &mut self.stream
    }

    pub(crate) fn protocol_minor(&self) -> u16 {
        self.protocol_minor
    }
}

impl AsRawFd for HolderDataConnection {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

fn exchange(
    stream: &mut UnixStream,
    request: HolderFrame,
    expected: HolderMessageType,
    protocol_minor: u16,
    validate: impl FnOnce(&[u8]) -> Result<()>,
) -> Result<()> {
    stream
        .write_all(
            &encode_frame_with_minor(&request, protocol_minor)
                .map_err(|error| protocol_error("request", error))?,
        )
        .map_err(|source| io_error("write holder data frame", source))?;
    let response = read_frame(stream, protocol_minor)?;
    if response.message_type != expected || response.request_id != request.request_id {
        return Err(PersistError::invalid_argument(
            "holder data response does not match request",
        ));
    }
    validate(&response.payload)
}

fn read_frame(stream: &mut UnixStream, protocol_minor: u16) -> Result<HolderFrame> {
    let mut header = [0u8; HOLDER_HEADER_SIZE];
    stream
        .read_exact(&mut header)
        .map_err(|source| io_error("read holder data header", source))?;
    let length = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    if length > MAX_HOLDER_CONTROL_FRAME {
        return Err(PersistError::invalid_argument(
            "holder data payload exceeds limit",
        ));
    }
    let mut payload = vec![0; length];
    stream
        .read_exact(&mut payload)
        .map_err(|source| io_error("read holder data payload", source))?;
    let mut bytes = header.to_vec();
    bytes.extend(payload);
    decode_frame_with_minor(&bytes, protocol_minor)
        .map_err(|error| protocol_error("response", error))
}

fn protocol_error(context: &str, error: HolderProtocolError) -> PersistError {
    PersistError::invalid_argument(format!("invalid holder data {context}: {error:?}"))
}

fn io_error(operation: &'static str, source: std::io::Error) -> PersistError {
    PersistError::Io { operation, source }
}
