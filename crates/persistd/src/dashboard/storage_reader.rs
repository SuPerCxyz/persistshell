use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use super::format::{
    crc32, decode_minute, decode_segment_header, MinuteRecord, RECORD_HEADER_SIZE,
    SEGMENT_HEADER_SIZE,
};
use super::storage::StorageLimits;
use super::storage_security::verify_private_file;

pub(super) fn next_sequence(paths: &[PathBuf], hour_start_ms: u64) -> u32 {
    paths
        .iter()
        .filter_map(|path| parse_segment_name(path))
        .filter(|(hour, _)| *hour == hour_start_ms)
        .map(|(_, sequence)| sequence)
        .max()
        .map_or(0, |sequence| sequence.saturating_add(1))
}

pub(super) fn parse_segment_name(path: &Path) -> Option<(u64, u32)> {
    let name = path.file_name()?.to_str()?;
    let value = name.strip_prefix("segment-")?.strip_suffix(".pmt")?;
    let (hour, sequence) = value.split_once('-')?;
    Some((hour.parse().ok()?, sequence.parse().ok()?))
}

pub(super) fn read_segment(
    path: &Path,
    limits: StorageLimits,
) -> io::Result<(Vec<MinuteRecord>, bool)> {
    verify_private_file(path)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let file_len = file.metadata()?.len();
    if file_len > limits.max_segment_bytes {
        return Err(invalid_data("metric segment exceeds size limit"));
    }
    let mut header = [0; SEGMENT_HEADER_SIZE];
    file.read_exact(&mut header)?;
    let segment = decode_segment_header(&header)
        .map_err(|_| invalid_data("invalid metric segment header"))?;
    let mut offset = SEGMENT_HEADER_SIZE as u64;
    let mut repaired = false;
    let mut records = Vec::new();
    while offset < file_len {
        let remaining = file_len - offset;
        if remaining < RECORD_HEADER_SIZE as u64 {
            file.set_len(offset)?;
            repaired = true;
            break;
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut record_header = [0; RECORD_HEADER_SIZE];
        file.read_exact(&mut record_header)?;
        let payload_len = u32::from_be_bytes(record_header[..4].try_into().unwrap()) as usize;
        let expected_crc = u32::from_be_bytes(record_header[4..8].try_into().unwrap());
        if payload_len > limits.max_record_bytes {
            return Err(invalid_data("metric record exceeds size limit"));
        }
        let framed_len = RECORD_HEADER_SIZE
            .checked_add(payload_len)
            .ok_or_else(|| invalid_data("metric record length overflow"))?;
        if remaining < framed_len as u64 {
            file.set_len(offset)?;
            repaired = true;
            break;
        }
        let mut payload = vec![0; payload_len];
        file.read_exact(&mut payload)?;
        if crc32(&payload) != expected_crc {
            return Err(invalid_data("metric record checksum mismatch"));
        }
        let record =
            decode_minute(&payload).map_err(|_| invalid_data("invalid metric record payload"))?;
        if record.timestamp_ms / 3_600_000 * 3_600_000 != segment.hour_start_ms {
            return Err(invalid_data("metric record is in the wrong hour segment"));
        }
        records.push(record);
        offset += framed_len as u64;
    }
    Ok((records, repaired))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
