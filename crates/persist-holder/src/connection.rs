use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};

use persist_core::{PersistError, Result};
use persist_ipc::holder::{
    encode_frame_with_minor, HolderFrame, HolderFrameAccumulator, HOLDER_PROTOCOL_BASELINE_MINOR,
};

use crate::socket::PeerConnection;

pub(crate) const MAX_PENDING_OUTPUT: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionRole {
    Pending,
    Control,
    Data,
}

pub(crate) struct Connection {
    pub(crate) peer: PeerConnection,
    pub(crate) role: ConnectionRole,
    pub(crate) attached_session: Option<u32>,
    protocol_minor: u16,
    accumulator: HolderFrameAccumulator,
    output: VecDeque<Vec<u8>>,
    output_offset: usize,
    output_bytes: usize,
    close_after_flush: bool,
}

impl Connection {
    pub(crate) fn new(peer: PeerConnection) -> Result<Self> {
        peer.stream
            .set_read_timeout(None)
            .and_then(|_| peer.stream.set_write_timeout(None))
            .and_then(|_| peer.stream.set_nonblocking(true))
            .map_err(|source| io_error("set holder peer nonblocking", source))?;
        Ok(Self {
            peer,
            role: ConnectionRole::Pending,
            attached_session: None,
            protocol_minor: HOLDER_PROTOCOL_BASELINE_MINOR,
            accumulator: HolderFrameAccumulator::new(),
            output: VecDeque::new(),
            output_offset: 0,
            output_bytes: 0,
            close_after_flush: false,
        })
    }

    pub(crate) fn fd(&self) -> RawFd {
        self.peer.as_raw_fd()
    }

    pub(crate) fn read_frames(&mut self) -> Result<(Vec<(u16, HolderFrame)>, bool)> {
        let mut frames = Vec::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut closed = false;
        loop {
            match self.peer.stream.read(&mut buffer) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(count) => self
                    .accumulator
                    .feed(&buffer[..count])
                    .map_err(protocol_error)?,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(source) => return Err(io_error("read holder peer", source)),
            }
            while let Some(frame) = self
                .accumulator
                .try_read_supported()
                .map_err(protocol_error)?
            {
                frames.push(frame);
            }
        }
        while let Some(frame) = self
            .accumulator
            .try_read_supported()
            .map_err(protocol_error)?
        {
            frames.push(frame);
        }
        Ok((frames, closed))
    }

    pub(crate) fn queue(&mut self, frame: HolderFrame) -> Result<()> {
        let bytes = encode_frame_with_minor(&frame, self.protocol_minor).map_err(protocol_error)?;
        let new_size = self
            .output_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| PersistError::invalid_argument("holder output queue overflow"))?;
        if new_size > MAX_PENDING_OUTPUT {
            return Err(PersistError::invalid_argument(
                "holder client output queue limit exceeded",
            ));
        }
        self.output_bytes = new_size;
        self.output.push_back(bytes);
        Ok(())
    }

    pub(crate) fn flush(&mut self) -> Result<bool> {
        while let Some(front) = self.output.front() {
            match self.peer.stream.write(&front[self.output_offset..]) {
                Ok(0) => return Ok(false),
                Ok(count) => {
                    self.output_offset += count;
                    self.output_bytes -= count;
                    if self.output_offset == front.len() {
                        self.output.pop_front();
                        self.output_offset = 0;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(source) => return Err(io_error("write holder peer", source)),
            }
        }
        Ok(!(self.close_after_flush && self.output.is_empty()))
    }

    pub(crate) fn wants_write(&self) -> bool {
        !self.output.is_empty()
    }

    pub(crate) fn close_after_flush(&mut self) {
        self.close_after_flush = true;
    }

    pub(crate) fn protocol_minor(&self) -> u16 {
        self.protocol_minor
    }

    pub(crate) fn set_protocol_minor(&mut self, minor: u16) {
        self.protocol_minor = minor;
    }
}

fn protocol_error(error: persist_ipc::holder::HolderProtocolError) -> PersistError {
    PersistError::invalid_argument(format!("invalid holder frame: {error:?}"))
}

fn io_error(operation: &'static str, source: io::Error) -> PersistError {
    PersistError::Io { operation, source }
}

#[cfg(test)]
mod tests {
    use super::*;
    use persist_ipc::holder::{HolderMessageType, HOLDER_HEADER_SIZE, MAX_HOLDER_IO_FRAME};
    use std::os::unix::net::UnixStream;

    #[test]
    fn output_queue_has_a_hard_limit() {
        let (stream, _peer) = UnixStream::pair().unwrap();
        let mut connection = Connection::new(PeerConnection {
            stream,
            pid: std::process::id(),
            uid: unsafe { libc::getuid() },
        })
        .unwrap();
        let frame = HolderFrame {
            message_type: HolderMessageType::Output,
            flags: 0,
            request_id: 0,
            generation: 0,
            payload: vec![0; MAX_HOLDER_IO_FRAME],
        };
        while connection.output_bytes + HOLDER_HEADER_SIZE + MAX_HOLDER_IO_FRAME
            <= MAX_PENDING_OUTPUT
        {
            connection.queue(frame.clone()).unwrap();
        }
        assert!(connection.queue(frame).is_err());
        assert!(connection.output_bytes <= MAX_PENDING_OUTPUT);
    }
}
