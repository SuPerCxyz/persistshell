use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc::TrySendError;
use std::time::Duration;

use super::format_tests::minute;
use super::storage::{MetricStorage, StorageLimits};
use super::writer::*;

#[test]
fn writer_queue_has_hard_capacity() {
    let (sender, _receiver) = std::sync::mpsc::sync_channel(WRITER_QUEUE_CAPACITY);
    sender.try_send(minute(1_000)).unwrap();
    sender.try_send(minute(2_000)).unwrap();
    assert!(matches!(
        sender.try_send(minute(3_000)),
        Err(TrySendError::Full(_))
    ));
}

#[test]
fn writer_loads_deduplicates_and_flushes_on_disconnect() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("metrics");
    let mut initial = MetricStorage::open(&path, StorageLimits::default()).unwrap();
    initial.append(&minute(60_000)).unwrap();
    drop(initial);

    let writer = spawn_writer(path.clone(), StorageLimits::default());
    writer.sender.send(minute(60_000)).unwrap();
    writer.sender.send(minute(120_000)).unwrap();
    writer.sender.send(minute(30_000)).unwrap();
    drop(writer.sender);
    writer
        .completion
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    writer.handle.join().unwrap();

    let status = *writer.status.lock().unwrap();
    assert!(status.available);
    assert_eq!(status.loaded_records, 1);
    let mut storage = MetricStorage::open(&path, StorageLimits::default()).unwrap();
    assert_eq!(storage.load().unwrap().records.len(), 3);
    assert_eq!(storage.segment_paths().len(), 2);
}

#[test]
fn unavailable_storage_drains_and_exits_without_panicking() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("metrics");
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let writer = spawn_writer(path, StorageLimits::default());
    let _ = writer.sender.send(minute(60_000));
    drop(writer.sender);
    writer
        .completion
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    writer.handle.join().unwrap();
    assert!(!writer.status.lock().unwrap().available);
}
