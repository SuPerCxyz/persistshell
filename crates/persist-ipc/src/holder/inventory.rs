use super::wire::{put_i32, put_u16, put_u32, put_u64, put_u8, Reader};
use super::{
    HolderLogState, HolderProtocolError, HolderSessionEntry, HolderSessionState, InventoryRequest,
    InventoryResponse, MAX_INVENTORY_ENTRIES,
};

const NO_CURSOR: u32 = u32::MAX;
const ENTRY_SIZE: usize = 37;

pub fn encode_inventory_request(request: &InventoryRequest) -> Vec<u8> {
    if request.cursor == NO_CURSOR || request.limit == 0 || request.limit > MAX_INVENTORY_ENTRIES {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(6);
    put_u32(&mut output, request.cursor);
    put_u16(&mut output, request.limit);
    output
}

pub fn decode_inventory_request(payload: &[u8]) -> Result<InventoryRequest, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let request = InventoryRequest {
        cursor: reader.u32()?,
        limit: reader.u16()?,
    };
    reader.finish()?;
    if request.cursor == NO_CURSOR || request.limit == 0 || request.limit > MAX_INVENTORY_ENTRIES {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(request)
}

pub fn encode_inventory_response(
    response: &InventoryResponse,
) -> Result<Vec<u8>, HolderProtocolError> {
    if response.entries.len() > MAX_INVENTORY_ENTRIES as usize {
        return Err(HolderProtocolError::PayloadTooLarge);
    }
    if matches!(response.next_cursor, Some(0 | NO_CURSOR)) {
        return Err(HolderProtocolError::InvalidField);
    }
    let capacity = 6usize
        .checked_add(response.entries.len().saturating_mul(ENTRY_SIZE))
        .ok_or(HolderProtocolError::PayloadTooLarge)?;
    let mut output = Vec::with_capacity(capacity);
    put_u32(&mut output, response.next_cursor.unwrap_or(NO_CURSOR));
    put_u16(&mut output, response.entries.len() as u16);
    for entry in &response.entries {
        encode_entry(&mut output, entry)?;
    }
    Ok(output)
}

pub fn decode_inventory_response(payload: &[u8]) -> Result<InventoryResponse, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let next_cursor = match reader.u32()? {
        NO_CURSOR => None,
        0 => return Err(HolderProtocolError::InvalidField),
        value => Some(value),
    };
    let count = reader.u16()?;
    if count > MAX_INVENTORY_ENTRIES {
        return Err(HolderProtocolError::PayloadTooLarge);
    }
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        entries.push(decode_entry(&mut reader)?);
    }
    reader.finish()?;
    Ok(InventoryResponse {
        entries,
        next_cursor,
    })
}

fn encode_entry(
    output: &mut Vec<u8>,
    entry: &HolderSessionEntry,
) -> Result<(), HolderProtocolError> {
    validate_entry(entry)?;
    put_u32(output, entry.session_id);
    put_u32(output, entry.shell_pid);
    put_u8(output, encode_state(entry.state));
    put_u8(output, u8::from(entry.exit_code.is_some()));
    put_i32(output, entry.exit_code.unwrap_or_default());
    put_u64(output, entry.created_at_ms);
    put_u64(output, entry.last_active_at_ms);
    put_u32(output, entry.ring_bytes);
    put_u8(output, u8::from(entry.writer_active));
    put_u8(output, encode_log_state(entry.log_state));
    put_u8(output, u8::from(entry.exit_context_available));
    Ok(())
}

fn decode_entry(reader: &mut Reader<'_>) -> Result<HolderSessionEntry, HolderProtocolError> {
    let session_id = reader.u32()?;
    let shell_pid = reader.u32()?;
    let state = decode_state(reader.u8()?)?;
    let has_exit = match reader.u8()? {
        0 => false,
        1 => true,
        _ => return Err(HolderProtocolError::InvalidField),
    };
    let raw_exit = reader.i32()?;
    let exit_code = has_exit.then_some(raw_exit);
    let entry = HolderSessionEntry {
        session_id,
        shell_pid,
        state,
        exit_code,
        created_at_ms: reader.u64()?,
        last_active_at_ms: reader.u64()?,
        ring_bytes: reader.u32()?,
        writer_active: match reader.u8()? {
            0 => false,
            1 => true,
            _ => return Err(HolderProtocolError::InvalidField),
        },
        log_state: decode_log_state(reader.u8()?)?,
        exit_context_available: match reader.u8()? {
            0 => false,
            1 => true,
            _ => return Err(HolderProtocolError::InvalidField),
        },
    };
    validate_entry(&entry)?;
    Ok(entry)
}

fn validate_entry(entry: &HolderSessionEntry) -> Result<(), HolderProtocolError> {
    let exit_matches_state = matches!(
        (entry.state, entry.exit_code),
        (HolderSessionState::Running, None) | (HolderSessionState::Exited, Some(_))
    );
    if entry.session_id == 0
        || entry.shell_pid == 0
        || !exit_matches_state
        || (entry.state == HolderSessionState::Running && entry.exit_context_available)
        || entry.last_active_at_ms < entry.created_at_ms
    {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(())
}

fn encode_state(state: HolderSessionState) -> u8 {
    match state {
        HolderSessionState::Running => 0,
        HolderSessionState::Exited => 1,
    }
}

fn decode_state(value: u8) -> Result<HolderSessionState, HolderProtocolError> {
    match value {
        0 => Ok(HolderSessionState::Running),
        1 => Ok(HolderSessionState::Exited),
        _ => Err(HolderProtocolError::InvalidField),
    }
}

fn encode_log_state(state: HolderLogState) -> u8 {
    match state {
        HolderLogState::Healthy => 0,
        HolderLogState::Degraded => 1,
        HolderLogState::Disabled => 2,
    }
}

fn decode_log_state(value: u8) -> Result<HolderLogState, HolderProtocolError> {
    match value {
        0 => Ok(HolderLogState::Healthy),
        1 => Ok(HolderLogState::Degraded),
        2 => Ok(HolderLogState::Disabled),
        _ => Err(HolderProtocolError::InvalidField),
    }
}
