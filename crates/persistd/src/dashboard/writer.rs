use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use super::format::MinuteRecord;
use super::storage::{LoadReport, MetricStorage, StorageLimits};

pub(super) const WRITER_QUEUE_CAPACITY: usize = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct WriterStatus {
    pub available: bool,
    pub loaded_records: usize,
    pub skipped_segments: usize,
    pub repaired_tails: usize,
    pub write_failures: u64,
}

pub(super) struct WriterThread {
    pub sender: SyncSender<WriterCommand>,
    pub status: Arc<Mutex<WriterStatus>>,
    pub completion: Receiver<()>,
    pub handle: JoinHandle<()>,
}

pub(super) enum WriterCommand {
    Append(MinuteRecord),
    Load(SyncSender<Option<LoadReport>>),
}

pub(super) fn spawn_writer(path: PathBuf, limits: StorageLimits) -> WriterThread {
    let (sender, receiver) = mpsc::sync_channel(WRITER_QUEUE_CAPACITY);
    let (completion_tx, completion) = mpsc::sync_channel(1);
    let status = Arc::new(Mutex::new(WriterStatus::default()));
    let thread_status = Arc::clone(&status);
    let handle = thread::spawn(move || {
        run_writer(path, limits, receiver, &thread_status);
        let _ = completion_tx.send(());
    });
    WriterThread {
        sender,
        status,
        completion,
        handle,
    }
}

fn run_writer(
    path: PathBuf,
    limits: StorageLimits,
    receiver: Receiver<WriterCommand>,
    status: &Mutex<WriterStatus>,
) {
    let Ok(mut storage) = MetricStorage::open(&path, limits) else {
        drain_unavailable(receiver);
        return;
    };
    let Ok(report) = storage.load() else {
        drain_unavailable(receiver);
        return;
    };
    let mut last_timestamp = report.records.last().map(|record| record.timestamp_ms);
    replace_status(
        status,
        WriterStatus {
            available: true,
            loaded_records: report.records.len(),
            skipped_segments: report.skipped_segments,
            repaired_tails: report.repaired_tails,
            write_failures: 0,
        },
    );
    for command in receiver {
        match command {
            WriterCommand::Append(record) => {
                if last_timestamp == Some(record.timestamp_ms) {
                    continue;
                }
                match storage.append(&record) {
                    Ok(()) => last_timestamp = Some(record.timestamp_ms),
                    Err(_) => record_failure(status),
                }
            }
            WriterCommand::Load(reply) => {
                let _ = reply.send(storage.load().ok());
            }
        }
    }
    if storage.flush().is_err() {
        record_failure(status);
    }
}

fn drain_unavailable(receiver: Receiver<WriterCommand>) {
    for command in receiver {
        if let WriterCommand::Load(reply) = command {
            let _ = reply.send(None);
        }
    }
}

fn replace_status(status: &Mutex<WriterStatus>, value: WriterStatus) {
    if let Ok(mut status) = status.lock() {
        *status = value;
    }
}

fn record_failure(status: &Mutex<WriterStatus>) {
    if let Ok(mut status) = status.lock() {
        status.available = false;
        status.write_failures = status.write_failures.saturating_add(1);
    }
}
