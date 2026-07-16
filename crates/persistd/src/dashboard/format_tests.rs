use persist_ipc::{Completeness, TrendPoint};

use super::format::*;

fn point(timestamp_ms: u64, value: u64) -> TrendPoint {
    TrendPoint {
        timestamp_ms,
        cpu_avg_milli_percent: value as u32,
        cpu_peak_milli_percent: value as u32 + 1,
        rss_avg_kib: value + 2,
        rss_peak_kib: value + 3,
        read_bytes: value + 4,
        write_bytes: value + 5,
        process_count_peak: 6,
        session_count: 7,
        runtime_count: 8,
        active_writer_count: 9,
        readonly_client_count: 10,
    }
}

pub(super) fn minute(timestamp_ms: u64) -> MinuteRecord {
    MinuteRecord {
        timestamp_ms,
        completeness: Completeness::Partial,
        daemon: point(timestamp_ms, 100),
        sessions: vec![SessionMinuteRecord {
            session_id: 42,
            completeness: Completeness::Complete,
            point: point(timestamp_ms, 200),
        }],
    }
}

#[test]
fn segment_header_round_trip_and_version_rejection() {
    let header = SegmentHeader {
        hour_start_ms: 3_600_000,
        sequence: 2,
    };
    let encoded = encode_segment_header(header);
    assert_eq!(decode_segment_header(&encoded), Ok(header));
    let mut unknown = encoded;
    unknown[8..10].copy_from_slice(&99_u16.to_be_bytes());
    assert_eq!(decode_segment_header(&unknown), Err(FormatError::Version));
    assert_eq!(
        decode_segment_header(&unknown[..5]),
        Err(FormatError::Truncated)
    );
}

#[test]
fn minute_payload_round_trip_and_bounds() {
    let record = minute(123_000);
    let encoded = encode_minute(&record).unwrap();
    assert_eq!(decode_minute(&encoded), Ok(record.clone()));
    assert_eq!(
        decode_minute(&encoded[..encoded.len() - 1]),
        Err(FormatError::Truncated)
    );
    let mut trailing = encoded;
    trailing.push(0);
    assert_eq!(decode_minute(&trailing), Err(FormatError::Trailing));

    let mut unknown_completeness = encode_minute(&record).unwrap();
    unknown_completeness[8] = 9;
    assert_eq!(
        decode_minute(&unknown_completeness),
        Err(FormatError::Invalid)
    );
}

#[test]
fn minute_payload_rejects_inconsistent_timestamps() {
    let mut record = minute(123_000);
    record.daemon.timestamp_ms += 1;
    assert_eq!(encode_minute(&record), Err(FormatError::Invalid));

    let mut encoded = encode_minute(&minute(123_000)).unwrap();
    encoded[9..17].copy_from_slice(&123_001_u64.to_be_bytes());
    assert_eq!(decode_minute(&encoded), Err(FormatError::Invalid));
}

#[test]
fn crc32_matches_standard_vector() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}
