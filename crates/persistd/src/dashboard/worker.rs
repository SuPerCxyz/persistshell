use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use persist_ipc::{Completeness, TrendScope};

use super::format::{MinuteRecord, SessionMinuteRecord};
use super::history::BoundedHistory;
use super::model::{derive_sample, RawDaemonSample, RawSample};
use super::proc_source::RealProcSource;
use super::procfs::{collect_procfs_until, ProcSource, SessionRoot, MAX_PROC_ENTRIES};
use super::storage::StorageLimits;
use super::writer::{spawn_writer, WriterCommand, WriterStatus, WriterThread};

pub(super) const SAMPLE_QUEUE_CAPACITY: usize = 1;
pub(crate) const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const SAMPLE_DEADLINE: Duration = Duration::from_secs(2);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SampleRequest {
    pub roots: Vec<SessionRoot>,
    pub session_count: u32,
    pub runtime_count: u32,
    pub active_writer_count: u32,
    pub readonly_client_count: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct WorkerStatus {
    pub running: bool,
    pub samples: u64,
    pub dropped_triggers: u64,
    pub dropped_writes: u64,
    pub panicked: bool,
}

pub(super) struct SharedDashboard {
    pub history: RwLock<BoundedHistory>,
    pub worker_status: Mutex<WorkerStatus>,
    pub writer_status: Arc<Mutex<WriterStatus>>,
}

pub(crate) struct DashboardRuntime {
    trigger: Option<SyncSender<SampleRequest>>,
    worker_completion: Receiver<()>,
    worker_handle: Option<JoinHandle<()>>,
    pub(super) writer: Option<WriterThread>,
    pub(super) shared: Arc<SharedDashboard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TriggerResult {
    Queued,
    Coalesced,
    Stopped,
}

impl DashboardRuntime {
    pub(crate) fn start(metrics_dir: PathBuf) -> Self {
        Self::start_with_source(
            metrics_dir,
            StorageLimits::default(),
            Box::new(RealProcSource::system()),
        )
    }

    pub(super) fn start_with_source(
        metrics_dir: PathBuf,
        limits: StorageLimits,
        source: Box<dyn ProcSource + Send>,
    ) -> Self {
        let writer = spawn_writer(metrics_dir, limits);
        let shared = Arc::new(SharedDashboard {
            history: RwLock::new(BoundedHistory::new()),
            worker_status: Mutex::new(WorkerStatus {
                running: true,
                ..WorkerStatus::default()
            }),
            writer_status: Arc::clone(&writer.status),
        });
        let (trigger, receiver) = mpsc::sync_channel(SAMPLE_QUEUE_CAPACITY);
        let (completion_tx, worker_completion) = mpsc::sync_channel(1);
        let worker_shared = Arc::clone(&shared);
        let writer_sender = writer.sender.clone();
        let worker_handle = thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_worker(source, receiver, writer_sender, &worker_shared)
            }));
            if let Ok(mut status) = worker_shared.worker_status.lock() {
                status.running = false;
                status.panicked = result.is_err();
            }
            let _ = completion_tx.send(());
        });
        Self {
            trigger: Some(trigger),
            worker_completion,
            worker_handle: Some(worker_handle),
            writer: Some(writer),
            shared,
        }
    }

    pub(crate) fn try_trigger(&self, request: SampleRequest) -> TriggerResult {
        let Some(trigger) = &self.trigger else {
            return TriggerResult::Stopped;
        };
        match trigger.try_send(request) {
            Ok(()) => TriggerResult::Queued,
            Err(TrySendError::Full(_)) => {
                update_worker_status_ref(&self.shared, |status| {
                    status.dropped_triggers = status.dropped_triggers.saturating_add(1);
                });
                TriggerResult::Coalesced
            }
            Err(TrySendError::Disconnected(_)) => TriggerResult::Stopped,
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.trigger.take();
        if self.worker_completion.recv_timeout(SHUTDOWN_WAIT).is_ok() {
            if let Some(handle) = self.worker_handle.take() {
                let _ = handle.join();
            }
        } else {
            self.worker_handle.take();
        }
        if let Some(writer) = self.writer.take() {
            shutdown_writer(writer);
        }
    }
}

impl Drop for DashboardRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_worker(
    source: Box<dyn ProcSource + Send>,
    receiver: Receiver<SampleRequest>,
    writer: SyncSender<WriterCommand>,
    shared: &SharedDashboard,
) {
    let daemon_pid = std::process::id();
    let page_size = sysconf(libc::_SC_PAGESIZE);
    let clock_ticks = sysconf(libc::_SC_CLK_TCK);
    let monotonic_start = Instant::now();
    let mut previous = None;
    let mut minute_start = None;
    loop {
        let request = match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(request) => request,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let started = Instant::now();
        let mut snapshot = collect_procfs_until(
            source.as_ref(),
            daemon_pid,
            &request.roots,
            page_size,
            MAX_PROC_ENTRIES,
            Some(started + SAMPLE_DEADLINE),
        );
        if started.elapsed() > SAMPLE_DEADLINE && snapshot.completeness == Completeness::Complete {
            snapshot.completeness = Completeness::Partial;
        }
        let sampled_at_ms = wall_time_ms();
        let raw = RawSample {
            sampled_at_ms,
            monotonic_ms: monotonic_start
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
            clock_ticks_per_second: clock_ticks,
            completeness: snapshot.completeness,
            daemon: RawDaemonSample {
                pid: daemon_pid,
                counters: snapshot.daemon.unwrap_or_default(),
                session_count: request.session_count,
                runtime_count: request.runtime_count,
                active_writer_count: request.active_writer_count,
                readonly_client_count: request.readonly_client_count,
            },
            sessions: snapshot.sessions,
        };
        let derived = derive_sample(previous.as_ref(), &raw);
        previous = Some(raw);
        let current_minute = sampled_at_ms / 60_000 * 60_000;
        let minute_record = {
            let Ok(mut history) = shared.history.write() else {
                continue;
            };
            history.push(derived);
            completed_minute(&history, &mut minute_start, current_minute)
        };
        update_worker_status_ref(shared, |status| {
            status.samples = status.samples.saturating_add(1);
        });
        if let Some(record) = minute_record {
            send_minute(&writer, record, shared);
        }
    }
}

fn sysconf(name: libc::c_int) -> u64 {
    let value = unsafe { libc::sysconf(name) };
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn wall_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn completed_minute(
    history: &BoundedHistory,
    minute_start: &mut Option<u64>,
    current_minute: u64,
) -> Option<MinuteRecord> {
    let Some(previous_minute) = *minute_start else {
        *minute_start = Some(current_minute);
        return None;
    };
    if current_minute <= previous_minute {
        if current_minute < previous_minute {
            *minute_start = Some(current_minute);
        }
        return None;
    }
    *minute_start = Some(current_minute);
    build_minute_record(history, previous_minute)
}

fn build_minute_record(history: &BoundedHistory, start_ms: u64) -> Option<MinuteRecord> {
    let end_ms = start_ms.saturating_add(60_000);
    let daemon_series = history.minute_series(TrendScope::Daemon, start_ms);
    let mut daemon = daemon_series.points.into_iter().next()?;
    daemon.timestamp_ms = end_ms;
    let mut completeness = daemon_series.completeness;
    let mut sessions = Vec::new();
    for session_id in history.session_ids_wall(start_ms, end_ms) {
        let series = history.minute_series(TrendScope::Session(session_id), start_ms);
        let Some(mut point) = series.points.into_iter().next() else {
            continue;
        };
        point.timestamp_ms = end_ms;
        if completeness == Completeness::Complete && series.completeness != Completeness::Complete {
            completeness = Completeness::Partial;
        }
        sessions.push(SessionMinuteRecord {
            session_id,
            completeness: series.completeness,
            point,
        });
    }
    Some(MinuteRecord {
        timestamp_ms: end_ms,
        completeness,
        daemon,
        sessions,
    })
}

fn update_worker_status_ref(shared: &SharedDashboard, update: impl FnOnce(&mut WorkerStatus)) {
    if let Ok(mut status) = shared.worker_status.lock() {
        update(&mut status);
    }
}

pub(super) fn send_minute(
    writer: &SyncSender<WriterCommand>,
    record: MinuteRecord,
    shared: &SharedDashboard,
) {
    if writer.try_send(WriterCommand::Append(record)).is_err() {
        update_worker_status_ref(shared, |status| {
            status.dropped_writes = status.dropped_writes.saturating_add(1);
        });
    }
}

fn shutdown_writer(writer: WriterThread) {
    drop(writer.sender);
    if writer.completion.recv_timeout(SHUTDOWN_WAIT).is_ok() {
        let _ = writer.handle.join();
    }
}
