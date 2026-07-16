use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::format_tests::minute;
use super::history::BoundedHistory;
use super::procfs::ProcSource;
use super::storage::StorageLimits;
use super::worker::*;
use super::writer::WriterCommand;

struct EmptySource;

impl ProcSource for EmptySource {
    fn list_pids(&self, _: usize) -> io::Result<(Vec<u32>, bool)> {
        Ok((Vec::new(), false))
    }

    fn read_stat(&self, _: u32) -> io::Result<String> {
        unreachable!()
    }

    fn read_io(&self, _: u32) -> io::Result<String> {
        unreachable!()
    }
}

struct BlockingSource {
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    calls: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

impl ProcSource for BlockingSource {
    fn list_pids(&self, _: usize) -> io::Result<(Vec<u32>, bool)> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        self.entered.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok((Vec::new(), false))
    }

    fn read_stat(&self, _: u32) -> io::Result<String> {
        unreachable!()
    }

    fn read_io(&self, _: u32) -> io::Result<String> {
        unreachable!()
    }
}

fn request() -> SampleRequest {
    SampleRequest {
        roots: Vec::new(),
        session_count: 0,
        runtime_count: 0,
        active_writer_count: 0,
        readonly_client_count: 0,
    }
}

#[test]
fn trigger_queue_coalesces_without_reentrant_scans() {
    let temp = tempfile::tempdir().unwrap();
    let (entered_tx, entered_rx) = mpsc::sync_channel(2);
    let (release_tx, release_rx) = mpsc::sync_channel(2);
    let calls = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let source = BlockingSource {
        entered: entered_tx,
        release: Mutex::new(release_rx),
        calls: Arc::clone(&calls),
        active,
        max_active: Arc::clone(&max_active),
    };
    let mut runtime = DashboardRuntime::start_with_source(
        temp.path().join("metrics"),
        StorageLimits::default(),
        Box::new(source),
    );

    assert_eq!(runtime.try_trigger(request()), TriggerResult::Queued);
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(runtime.try_trigger(request()), TriggerResult::Queued);
    assert_eq!(runtime.try_trigger(request()), TriggerResult::Coalesced);
    release_tx.send(()).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    release_tx.send(()).unwrap();
    runtime.shutdown();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(max_active.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime
            .shared
            .worker_status
            .lock()
            .unwrap()
            .dropped_triggers,
        1
    );
}

#[test]
fn shutdown_drains_queued_sample_and_stops_both_threads() {
    let temp = tempfile::tempdir().unwrap();
    let mut runtime = DashboardRuntime::start_with_source(
        temp.path().join("metrics"),
        StorageLimits::default(),
        Box::new(EmptySource),
    );
    assert_eq!(runtime.try_trigger(request()), TriggerResult::Queued);
    runtime.shutdown();
    assert_eq!(runtime.shared.history.read().unwrap().len(), 1);
    let status = *runtime.shared.worker_status.lock().unwrap();
    assert_eq!(status.samples, 1);
    assert!(!status.running);
    assert!(!status.panicked);
    assert!(runtime.shared.writer_status.lock().unwrap().available);
}

struct PanicSource;

impl ProcSource for PanicSource {
    fn list_pids(&self, _: usize) -> io::Result<(Vec<u32>, bool)> {
        panic!("injected collector panic")
    }

    fn read_stat(&self, _: u32) -> io::Result<String> {
        unreachable!()
    }

    fn read_io(&self, _: u32) -> io::Result<String> {
        unreachable!()
    }
}

#[test]
fn collector_panic_is_contained_and_reported() {
    let temp = tempfile::tempdir().unwrap();
    let mut runtime = DashboardRuntime::start_with_source(
        temp.path().join("metrics"),
        StorageLimits::default(),
        Box::new(PanicSource),
    );
    runtime.try_trigger(request());
    runtime.shutdown();
    let status = *runtime.shared.worker_status.lock().unwrap();
    assert!(status.panicked);
    assert!(!status.running);
}

#[test]
fn full_writer_queue_drops_batch_and_updates_status() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    sender
        .try_send(WriterCommand::Append(minute(60_000)))
        .unwrap();
    let shared = SharedDashboard {
        history: std::sync::RwLock::new(BoundedHistory::new()),
        worker_status: Mutex::new(WorkerStatus::default()),
        writer_status: Arc::new(Mutex::new(Default::default())),
    };
    send_minute(&sender, minute(120_000), &shared);
    assert_eq!(shared.worker_status.lock().unwrap().dropped_writes, 1);
}
