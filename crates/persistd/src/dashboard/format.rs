use std::collections::HashSet;

use persist_ipc::{Completeness, TrendPoint};

pub(super) const SEGMENT_HEADER_SIZE: usize = 24;
pub(super) const RECORD_HEADER_SIZE: usize = 8;
const MAGIC: &[u8; 8] = b"PSMETRIC";
const VERSION: u16 = 1;
const SESSION_POINT_SIZE: usize = 73;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FormatError {
    Truncated,
    Trailing,
    Magic,
    Version,
    Invalid,
    Limit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SegmentHeader {
    pub hour_start_ms: u64,
    pub sequence: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionMinuteRecord {
    pub session_id: u32,
    pub completeness: Completeness,
    pub point: TrendPoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MinuteRecord {
    pub timestamp_ms: u64,
    pub completeness: Completeness,
    pub daemon: TrendPoint,
    pub sessions: Vec<SessionMinuteRecord>,
}

pub(super) fn encode_segment_header(header: SegmentHeader) -> [u8; SEGMENT_HEADER_SIZE] {
    let mut output = [0; SEGMENT_HEADER_SIZE];
    output[..8].copy_from_slice(MAGIC);
    output[8..10].copy_from_slice(&VERSION.to_be_bytes());
    output[12..20].copy_from_slice(&header.hour_start_ms.to_be_bytes());
    output[20..24].copy_from_slice(&header.sequence.to_be_bytes());
    output
}

pub(super) fn decode_segment_header(payload: &[u8]) -> Result<SegmentHeader, FormatError> {
    if payload.len() < SEGMENT_HEADER_SIZE {
        return Err(FormatError::Truncated);
    }
    if payload.len() > SEGMENT_HEADER_SIZE {
        return Err(FormatError::Trailing);
    }
    if &payload[..8] != MAGIC {
        return Err(FormatError::Magic);
    }
    if u16::from_be_bytes(payload[8..10].try_into().unwrap()) != VERSION {
        return Err(FormatError::Version);
    }
    if payload[10..12] != [0, 0] {
        return Err(FormatError::Invalid);
    }
    Ok(SegmentHeader {
        hour_start_ms: u64::from_be_bytes(payload[12..20].try_into().unwrap()),
        sequence: u32::from_be_bytes(payload[20..24].try_into().unwrap()),
    })
}

pub(super) fn crc32(payload: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in payload {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

pub(super) fn encode_minute(record: &MinuteRecord) -> Result<Vec<u8>, FormatError> {
    if record.sessions.len() > u16::MAX as usize {
        return Err(FormatError::Limit);
    }
    if !timestamps_match(record) {
        return Err(FormatError::Invalid);
    }
    let mut seen = HashSet::with_capacity(record.sessions.len());
    if record
        .sessions
        .iter()
        .any(|session| session.session_id == 0 || !seen.insert(session.session_id))
    {
        return Err(FormatError::Invalid);
    }
    let mut output = Vec::new();
    put_u64(&mut output, record.timestamp_ms);
    put_u8(&mut output, encode_completeness(record.completeness));
    encode_point(&mut output, record.daemon);
    put_u16(&mut output, record.sessions.len() as u16);
    for session in &record.sessions {
        put_u32(&mut output, session.session_id);
        put_u8(&mut output, encode_completeness(session.completeness));
        encode_point(&mut output, session.point);
    }
    Ok(output)
}

pub(super) fn decode_minute(payload: &[u8]) -> Result<MinuteRecord, FormatError> {
    let mut reader = Reader::new(payload);
    let timestamp_ms = reader.u64()?;
    let completeness = decode_completeness(reader.u8()?)?;
    let daemon = decode_point(&mut reader)?;
    let count = reader.u16()?;
    let expected = usize::from(count)
        .checked_mul(SESSION_POINT_SIZE)
        .and_then(|bytes| reader.offset.checked_add(bytes))
        .ok_or(FormatError::Limit)?;
    if expected > payload.len() {
        return Err(FormatError::Truncated);
    }
    let mut sessions = Vec::with_capacity(usize::from(count));
    let mut seen = HashSet::with_capacity(usize::from(count));
    for _ in 0..count {
        let session_id = reader.u32()?;
        if session_id == 0 || !seen.insert(session_id) {
            return Err(FormatError::Invalid);
        }
        sessions.push(SessionMinuteRecord {
            session_id,
            completeness: decode_completeness(reader.u8()?)?,
            point: decode_point(&mut reader)?,
        });
    }
    if !reader.finish() {
        return Err(FormatError::Trailing);
    }
    let record = MinuteRecord {
        timestamp_ms,
        completeness,
        daemon,
        sessions,
    };
    if !timestamps_match(&record) {
        return Err(FormatError::Invalid);
    }
    Ok(record)
}

fn timestamps_match(record: &MinuteRecord) -> bool {
    record.daemon.timestamp_ms == record.timestamp_ms
        && record
            .sessions
            .iter()
            .all(|session| session.point.timestamp_ms == record.timestamp_ms)
}

pub(super) fn frame_record(
    record: &MinuteRecord,
    max_bytes: usize,
) -> Result<Vec<u8>, FormatError> {
    let payload = encode_minute(record)?;
    if payload.len() > max_bytes || payload.len() > u32::MAX as usize {
        return Err(FormatError::Limit);
    }
    let mut output = Vec::with_capacity(RECORD_HEADER_SIZE + payload.len());
    put_u32(&mut output, payload.len() as u32);
    put_u32(&mut output, crc32(&payload));
    output.extend_from_slice(&payload);
    Ok(output)
}

fn encode_point(output: &mut Vec<u8>, point: TrendPoint) {
    put_u64(output, point.timestamp_ms);
    put_u32(output, point.cpu_avg_milli_percent);
    put_u32(output, point.cpu_peak_milli_percent);
    put_u64(output, point.rss_avg_kib);
    put_u64(output, point.rss_peak_kib);
    put_u64(output, point.read_bytes);
    put_u64(output, point.write_bytes);
    put_u32(output, point.process_count_peak);
    put_u32(output, point.session_count);
    put_u32(output, point.runtime_count);
    put_u32(output, point.active_writer_count);
    put_u32(output, point.readonly_client_count);
}

fn decode_point(reader: &mut Reader<'_>) -> Result<TrendPoint, FormatError> {
    Ok(TrendPoint {
        timestamp_ms: reader.u64()?,
        cpu_avg_milli_percent: reader.u32()?,
        cpu_peak_milli_percent: reader.u32()?,
        rss_avg_kib: reader.u64()?,
        rss_peak_kib: reader.u64()?,
        read_bytes: reader.u64()?,
        write_bytes: reader.u64()?,
        process_count_peak: reader.u32()?,
        session_count: reader.u32()?,
        runtime_count: reader.u32()?,
        active_writer_count: reader.u32()?,
        readonly_client_count: reader.u32()?,
    })
}

fn encode_completeness(value: Completeness) -> u8 {
    match value {
        Completeness::Complete => 0,
        Completeness::Partial => 1,
        Completeness::Stale => 2,
        Completeness::Unavailable => 3,
    }
}

fn decode_completeness(value: u8) -> Result<Completeness, FormatError> {
    match value {
        0 => Ok(Completeness::Complete),
        1 => Ok(Completeness::Partial),
        2 => Ok(Completeness::Stale),
        3 => Ok(Completeness::Unavailable),
        _ => Err(FormatError::Invalid),
    }
}

fn put_u8(output: &mut Vec<u8>, value: u8) {
    output.push(value);
}

fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

struct Reader<'a> {
    payload: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(payload: &'a [u8]) -> Self {
        Self { payload, offset: 0 }
    }

    fn u8(&mut self) -> Result<u8, FormatError> {
        let value = *self
            .payload
            .get(self.offset)
            .ok_or(FormatError::Truncated)?;
        self.offset += 1;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_be_bytes(self.take::<2>()?))
    }

    fn u32(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_be_bytes(self.take::<4>()?))
    }

    fn u64(&mut self) -> Result<u64, FormatError> {
        Ok(u64::from_be_bytes(self.take::<8>()?))
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], FormatError> {
        let end = self.offset.checked_add(N).ok_or(FormatError::Limit)?;
        let value = self
            .payload
            .get(self.offset..end)
            .ok_or(FormatError::Truncated)?
            .try_into()
            .map_err(|_| FormatError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn finish(&self) -> bool {
        self.offset == self.payload.len()
    }
}
