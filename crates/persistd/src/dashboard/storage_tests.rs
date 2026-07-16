use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{symlink, PermissionsExt};

use super::format_tests::minute;
use super::storage::*;

fn limits() -> StorageLimits {
    StorageLimits {
        max_segments: 24,
        max_total_bytes: 128 * 1024 * 1024,
        max_segment_bytes: 128 * 1024 * 1024,
        max_record_bytes: 1024 * 1024,
        max_directory_entries: 32,
    }
}

fn open_temp(temp: &tempfile::TempDir, limits: StorageLimits) -> MetricStorage {
    MetricStorage::open(&temp.path().join("metrics"), limits).unwrap()
}

#[test]
fn open_creates_private_directory_and_append_survives_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("metrics");
    {
        let mut storage = MetricStorage::open(&path, limits()).unwrap();
        storage.append(&minute(1_000)).unwrap();
    }
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let mut storage = MetricStorage::open(&path, limits()).unwrap();
    storage.append(&minute(2_000)).unwrap();
    assert_eq!(storage.segment_paths().len(), 1);
    let report = storage.load().unwrap();
    assert_eq!(report.records, vec![minute(1_000), minute(2_000)]);
    assert_eq!(report.skipped_segments, 0);
}

#[test]
fn open_rejects_symlink_and_broad_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir(&target).unwrap();
    let link = temp.path().join("link");
    symlink(&target, &link).unwrap();
    assert!(MetricStorage::open(&link, limits()).is_err());

    let broad = temp.path().join("broad");
    fs::create_dir(&broad).unwrap();
    fs::set_permissions(&broad, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(MetricStorage::open(&broad, limits()).is_err());

    let private = temp.path().join("private");
    let mut storage = MetricStorage::open(&private, limits()).unwrap();
    storage.append(&minute(1_000)).unwrap();
    let segment = storage.segment_paths().pop().unwrap();
    fs::set_permissions(&segment, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(MetricStorage::open(&private, limits()).is_err());
}

#[test]
fn incomplete_tail_is_truncated_and_prior_records_survive() {
    let temp = tempfile::tempdir().unwrap();
    let mut storage = open_temp(&temp, limits());
    storage.append(&minute(1_000)).unwrap();
    let path = storage.segment_paths().pop().unwrap();
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&[0, 0, 0])
        .unwrap();
    let damaged_len = fs::metadata(&path).unwrap().len();
    let report = storage.load().unwrap();
    assert_eq!(report.records, vec![minute(1_000)]);
    assert_eq!(report.repaired_tails, 1);
    assert!(fs::metadata(path).unwrap().len() < damaged_len);
}

#[test]
fn crc_corruption_and_unknown_version_skip_segment() {
    let temp = tempfile::tempdir().unwrap();
    let mut storage = open_temp(&temp, limits());
    storage.append(&minute(1_000)).unwrap();
    let path = storage.segment_paths().pop().unwrap();
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    let mut byte = [0];
    file.read_exact(&mut byte).unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    file.write_all(&[byte[0] ^ 0xFF]).unwrap();
    assert_eq!(storage.load().unwrap().skipped_segments, 1);

    file.seek(SeekFrom::Start(8)).unwrap();
    file.write_all(&99_u16.to_be_bytes()).unwrap();
    assert_eq!(storage.load().unwrap().skipped_segments, 1);
}

#[test]
fn rotation_keeps_newest_segments_and_honors_total_size() {
    let temp = tempfile::tempdir().unwrap();
    let mut small = limits();
    let framed_size = super::format::frame_record(&minute(1_000), small.max_record_bytes)
        .unwrap()
        .len() as u64;
    small.max_total_bytes = 2 * (super::format::SEGMENT_HEADER_SIZE as u64 + framed_size);
    let mut storage = open_temp(&temp, small);
    for timestamp in [1_000, 3_601_000, 7_201_000] {
        storage.append(&minute(timestamp)).unwrap();
    }
    assert_eq!(storage.segment_paths().len(), 2);
    assert_eq!(
        storage.load().unwrap().records,
        vec![minute(3_601_000), minute(7_201_000)]
    );
    let total = storage
        .segment_paths()
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum::<u64>();
    assert!(total <= small.max_total_bytes);
}

#[test]
fn wall_clock_rollback_starts_new_sequence() {
    let temp = tempfile::tempdir().unwrap();
    let mut storage = open_temp(&temp, limits());
    storage.append(&minute(3_700_000)).unwrap();
    storage.append(&minute(3_650_000)).unwrap();
    assert_eq!(storage.segment_paths().len(), 2);
    assert_eq!(storage.load().unwrap().records.len(), 2);
}

#[test]
fn directory_entry_and_record_limits_are_enforced() {
    let temp = tempfile::tempdir().unwrap();
    let mut small = limits();
    small.max_directory_entries = 1;
    small.max_segments = 1;
    let metrics = temp.path().join("metrics");
    fs::create_dir(&metrics).unwrap();
    fs::set_permissions(&metrics, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(metrics.join("unexpected"), b"x").unwrap();
    fs::write(metrics.join("another"), b"x").unwrap();
    assert!(MetricStorage::open(&metrics, small).is_err());

    let empty = tempfile::tempdir().unwrap();
    let mut tiny = limits();
    tiny.max_record_bytes = 8;
    let mut storage = open_temp(&empty, tiny);
    assert!(storage.append(&minute(1_000)).is_err());
}
