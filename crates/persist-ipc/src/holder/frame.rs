use super::wire::{put_u16, put_u32, put_u64, Reader};
use super::{
    HolderFrame, HolderMessageType, HolderProtocolError, HOLDER_HEADER_SIZE, HOLDER_MAGIC,
    HOLDER_PROTOCOL_BASELINE_MINOR, HOLDER_PROTOCOL_MAJOR, HOLDER_PROTOCOL_MINOR,
    MAX_HOLDER_ACCUMULATOR, MAX_HOLDER_CONTROL_FRAME,
};

#[derive(Debug, Default)]
pub struct HolderFrameAccumulator {
    buffer: Vec<u8>,
}

impl HolderFrameAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), HolderProtocolError> {
        let new_len = self
            .buffer
            .len()
            .checked_add(bytes.len())
            .ok_or(HolderProtocolError::PayloadTooLarge)?;
        if new_len > MAX_HOLDER_ACCUMULATOR {
            return Err(HolderProtocolError::PayloadTooLarge);
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    pub fn try_read(&mut self) -> Result<Option<HolderFrame>, HolderProtocolError> {
        self.try_read_with_minor(HOLDER_PROTOCOL_BASELINE_MINOR)
    }

    pub fn try_read_with_minor(
        &mut self,
        minor: u16,
    ) -> Result<Option<HolderFrame>, HolderProtocolError> {
        self.try_read_supported().and_then(|value| match value {
            Some((actual, frame)) if actual == minor => Ok(Some(frame)),
            Some(_) => Err(HolderProtocolError::VersionMismatch),
            None => Ok(None),
        })
    }

    pub fn try_read_supported(
        &mut self,
    ) -> Result<Option<(u16, HolderFrame)>, HolderProtocolError> {
        if self.buffer.len() < HOLDER_HEADER_SIZE {
            return Ok(None);
        }
        let payload_len = u32::from_be_bytes(
            self.buffer[8..12]
                .try_into()
                .map_err(|_| HolderProtocolError::Truncated)?,
        ) as usize;
        if payload_len > MAX_HOLDER_CONTROL_FRAME {
            return Err(HolderProtocolError::PayloadTooLarge);
        }
        let frame_len = HOLDER_HEADER_SIZE
            .checked_add(payload_len)
            .ok_or(HolderProtocolError::PayloadTooLarge)?;
        if self.buffer.len() < frame_len {
            return Ok(None);
        }
        let minor = u16::from_be_bytes(
            self.buffer[6..8]
                .try_into()
                .map_err(|_| HolderProtocolError::Truncated)?,
        );
        let frame = decode_frame_with_minor(&self.buffer[..frame_len], minor)?;
        self.buffer.drain(..frame_len);
        Ok(Some((minor, frame)))
    }
}

pub fn encode_frame(frame: &HolderFrame) -> Result<Vec<u8>, HolderProtocolError> {
    encode_frame_with_minor(frame, HOLDER_PROTOCOL_BASELINE_MINOR)
}

pub fn encode_frame_with_minor(
    frame: &HolderFrame,
    minor: u16,
) -> Result<Vec<u8>, HolderProtocolError> {
    validate_minor(minor)?;
    if frame.flags != 0 {
        return Err(HolderProtocolError::InvalidField);
    }
    if frame.payload.len() > frame.message_type.max_payload() {
        return Err(HolderProtocolError::PayloadTooLarge);
    }
    let payload_len =
        u32::try_from(frame.payload.len()).map_err(|_| HolderProtocolError::PayloadTooLarge)?;
    let mut output = Vec::with_capacity(HOLDER_HEADER_SIZE + frame.payload.len());
    output.extend_from_slice(&HOLDER_MAGIC);
    put_u16(&mut output, HOLDER_PROTOCOL_MAJOR);
    put_u16(&mut output, minor);
    put_u32(&mut output, payload_len);
    put_u16(&mut output, frame.message_type as u16);
    put_u16(&mut output, frame.flags);
    put_u32(&mut output, frame.request_id);
    put_u64(&mut output, frame.generation);
    put_u32(&mut output, 0);
    output.extend_from_slice(&frame.payload);
    Ok(output)
}

pub fn decode_frame(bytes: &[u8]) -> Result<HolderFrame, HolderProtocolError> {
    decode_frame_with_minor(bytes, HOLDER_PROTOCOL_BASELINE_MINOR)
}

pub fn decode_frame_with_minor(
    bytes: &[u8],
    expected_minor: u16,
) -> Result<HolderFrame, HolderProtocolError> {
    validate_minor(expected_minor)?;
    if bytes.len() < HOLDER_HEADER_SIZE {
        return Err(HolderProtocolError::Truncated);
    }
    let mut reader = Reader::new(bytes);
    if reader.fixed::<4>()? != HOLDER_MAGIC {
        return Err(HolderProtocolError::InvalidMagic);
    }
    if reader.u16()? != HOLDER_PROTOCOL_MAJOR || reader.u16()? != expected_minor {
        return Err(HolderProtocolError::VersionMismatch);
    }
    let payload_len = reader.u32()? as usize;
    let message_type = HolderMessageType::from_u16(reader.u16()?)
        .ok_or(HolderProtocolError::UnknownMessageType)?;
    let flags = reader.u16()?;
    let request_id = reader.u32()?;
    let generation = reader.u64()?;
    if flags != 0 || reader.u32()? != 0 {
        return Err(HolderProtocolError::InvalidField);
    }
    if payload_len > message_type.max_payload() {
        return Err(HolderProtocolError::PayloadTooLarge);
    }
    let expected = HOLDER_HEADER_SIZE
        .checked_add(payload_len)
        .ok_or(HolderProtocolError::PayloadTooLarge)?;
    if bytes.len() < expected {
        return Err(HolderProtocolError::Truncated);
    }
    if bytes.len() > expected {
        return Err(HolderProtocolError::Trailing);
    }
    Ok(HolderFrame {
        message_type,
        flags,
        request_id,
        generation,
        payload: bytes[HOLDER_HEADER_SIZE..].to_vec(),
    })
}

fn validate_minor(minor: u16) -> Result<(), HolderProtocolError> {
    if (HOLDER_PROTOCOL_BASELINE_MINOR..=HOLDER_PROTOCOL_MINOR).contains(&minor) {
        Ok(())
    } else {
        Err(HolderProtocolError::VersionMismatch)
    }
}
