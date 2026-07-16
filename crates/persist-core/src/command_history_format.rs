use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::command_history::{
    io_error, validate_record, CommandRecord, HEADER_LEN, MAGIC, MAX_COMMAND_BYTES,
    MAX_HISTORY_BYTES, MAX_HISTORY_RECORDS, RECORD_META_LEN,
};
use crate::{PersistError, Result};

pub(crate) struct HistoryState {
    pub next_sequence: u64,
    pub records: Vec<CommandRecord>,
}

pub(crate) fn load_state(file: &mut File) -> Result<HistoryState> {
    let size = file
        .metadata()
        .map_err(|source| io_error("inspect command history", source))?
        .len();
    if size > MAX_HISTORY_BYTES {
        return Err(PersistError::invalid_argument(
            "command history exceeds size limit",
        ));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|source| io_error("seek command history", source))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| io_error("read command history", source))?;
    if bytes.is_empty() {
        return Ok(HistoryState {
            next_sequence: 1,
            records: Vec::new(),
        });
    }
    if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
        return Err(PersistError::invalid_argument(
            "invalid command history header",
        ));
    }
    let stored_next = read_u64(&bytes[8..16]);
    let stored_count = read_u64(&bytes[16..24]);
    let mut records = Vec::new();
    let mut cursor = HEADER_LEN;
    while cursor < bytes.len() {
        let (record, consumed) = decode_record(&bytes[cursor..])?;
        records.push(record);
        cursor += consumed;
    }
    validate_header(stored_count, &records)?;
    let parsed_next = records
        .last()
        .map(|record| record.sequence.saturating_add(1))
        .unwrap_or(1);
    Ok(HistoryState {
        next_sequence: stored_next.max(parsed_next),
        records,
    })
}

fn validate_header(stored_count: u64, records: &[CommandRecord]) -> Result<()> {
    if stored_count > records.len() as u64
        || (records.len() as u64).saturating_sub(stored_count) > 1
    {
        return Err(PersistError::invalid_argument(
            "command history record count does not match header",
        ));
    }
    if records
        .windows(2)
        .any(|pair| pair[0].sequence >= pair[1].sequence)
    {
        return Err(PersistError::invalid_argument(
            "command history sequence is invalid",
        ));
    }
    Ok(())
}

fn decode_record(bytes: &[u8]) -> Result<(CommandRecord, usize)> {
    if bytes.len() < 4 {
        return Err(PersistError::invalid_argument("truncated history record"));
    }
    let body_len = read_u32(&bytes[..4]) as usize;
    if body_len < RECORD_META_LEN
        || body_len > RECORD_META_LEN + u16::MAX as usize + MAX_COMMAND_BYTES
    {
        return Err(PersistError::invalid_argument(
            "invalid history record size",
        ));
    }
    if bytes.len() < 4 + body_len {
        return Err(PersistError::invalid_argument("truncated history record"));
    }
    let body = &bytes[4..4 + body_len];
    let shell_len = read_u16(&body[16..18]) as usize;
    if RECORD_META_LEN + shell_len > body.len() {
        return Err(PersistError::invalid_argument("invalid history shell size"));
    }
    let shell = std::str::from_utf8(&body[RECORD_META_LEN..RECORD_META_LEN + shell_len])
        .map_err(|_| PersistError::invalid_argument("history shell is not UTF-8"))?;
    let command = body[RECORD_META_LEN + shell_len..].to_vec();
    validate_record(shell, &command)?;
    Ok((
        CommandRecord {
            sequence: read_u64(&body[..8]),
            completed_at_ms: read_u64(&body[8..16]),
            shell: shell.to_string(),
            command,
        },
        4 + body_len,
    ))
}

pub(crate) fn write_state(file: &mut File, state: &HistoryState) -> Result<()> {
    let mut bytes = Vec::with_capacity(estimated_size(&state.records));
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&state.next_sequence.to_be_bytes());
    bytes.extend_from_slice(&(state.records.len() as u64).to_be_bytes());
    for record in &state.records {
        encode_record(record, &mut bytes)?;
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|source| io_error("seek command history", source))?;
    file.set_len(0)
        .map_err(|source| io_error("truncate command history", source))?;
    file.write_all(&bytes)
        .map_err(|source| io_error("write command history", source))?;
    file.sync_data()
        .map_err(|source| io_error("sync command history", source))
}

pub(crate) fn encode_record(record: &CommandRecord, output: &mut Vec<u8>) -> Result<()> {
    validate_record(&record.shell, &record.command)?;
    let body_len = RECORD_META_LEN + record.shell.len() + record.command.len();
    output.extend_from_slice(&(body_len as u32).to_be_bytes());
    output.extend_from_slice(&record.sequence.to_be_bytes());
    output.extend_from_slice(&record.completed_at_ms.to_be_bytes());
    output.extend_from_slice(&(record.shell.len() as u16).to_be_bytes());
    output.extend_from_slice(&0u16.to_be_bytes());
    output.extend_from_slice(record.shell.as_bytes());
    output.extend_from_slice(&record.command);
    Ok(())
}

pub(crate) fn compact(records: &mut Vec<CommandRecord>) {
    let mut first = records.len().saturating_sub(MAX_HISTORY_RECORDS);
    while estimated_size(&records[first..]) as u64 > MAX_HISTORY_BYTES && first < records.len() {
        first += 1;
    }
    if first > 0 {
        records.drain(..first);
    }
}

pub(crate) fn estimated_size(records: &[CommandRecord]) -> usize {
    HEADER_LEN
        + records
            .iter()
            .map(|record| 4 + RECORD_META_LEN + record.shell.len() + record.command.len())
            .sum::<usize>()
}

fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes(bytes.try_into().expect("two byte slice"))
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("four byte slice"))
}

fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().expect("eight byte slice"))
}
