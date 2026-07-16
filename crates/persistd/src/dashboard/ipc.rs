use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use persist_ipc::{
    Completeness, DaemonMetrics, DashboardSummaryRequest, DashboardSummaryResponse,
    DashboardTrendRequest, DashboardTrendResponse, SessionMetrics, TrendRange, MAX_SUMMARY_PAGE,
    MAX_TREND_POINTS,
};

use super::ipc_disk::aggregate_disk_trend;
use super::worker::{DashboardRuntime, SharedDashboard, SAMPLE_INTERVAL};
use super::writer::WriterCommand;

const DISK_QUERY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DashboardQueryError {
    InvalidRequest,
    InvalidCursor,
    Unavailable,
}

#[derive(Clone)]
pub(crate) struct DashboardService {
    shared: Arc<SharedDashboard>,
    writer: SyncSender<WriterCommand>,
}

impl DashboardService {
    pub(super) fn new(shared: Arc<SharedDashboard>, writer: SyncSender<WriterCommand>) -> Self {
        Self { shared, writer }
    }

    pub(crate) fn summary(
        &self,
        request: DashboardSummaryRequest,
    ) -> Result<DashboardSummaryResponse, DashboardQueryError> {
        if request.limit == 0 || request.limit > MAX_SUMMARY_PAGE {
            return Err(DashboardQueryError::InvalidRequest);
        }
        let history = self
            .shared
            .history
            .read()
            .map_err(|_| DashboardQueryError::Unavailable)?;
        let Some(latest) = history.latest() else {
            return request
                .cursor
                .eq(&0)
                .then(unavailable_summary)
                .ok_or(DashboardQueryError::InvalidCursor);
        };
        let mut sessions = latest
            .sessions
            .iter()
            .map(|session| session.metrics)
            .collect::<Vec<_>>();
        sessions.sort_unstable_by_key(|session| session.session_id);
        let start = page_start(&sessions, request.cursor)?;
        let end = start
            .saturating_add(usize::from(request.limit))
            .min(sessions.len());
        let next_cursor = (end < sessions.len()).then(|| sessions[end - 1].session_id);
        Ok(DashboardSummaryResponse {
            sampled_at_ms: latest.sampled_at_ms,
            completeness: summary_completeness(
                &self.shared,
                latest.sampled_at_ms,
                latest.completeness,
            ),
            daemon: latest.daemon.metrics,
            sessions: sessions[start..end].to_vec(),
            next_cursor,
        })
    }

    pub(crate) fn trend(
        &self,
        request: DashboardTrendRequest,
    ) -> Result<DashboardTrendResponse, DashboardQueryError> {
        if request.max_points == 0 || request.max_points > MAX_TREND_POINTS {
            return Err(DashboardQueryError::InvalidRequest);
        }
        match request.range {
            TrendRange::FifteenMinutes => self.memory_trend(request, 15 * 60 * 1_000),
            TrendRange::Hour => self.memory_trend(request, 60 * 60 * 1_000),
            TrendRange::Day => self.disk_trend(request),
        }
    }

    fn memory_trend(
        &self,
        request: DashboardTrendRequest,
        window_ms: u64,
    ) -> Result<DashboardTrendResponse, DashboardQueryError> {
        let history = self
            .shared
            .history
            .read()
            .map_err(|_| DashboardQueryError::Unavailable)?;
        let Some(latest) = history.latest() else {
            return Ok(unavailable_trend());
        };
        let series = history.trend(
            request.scope,
            latest.monotonic_ms,
            window_ms,
            request.max_points,
        );
        Ok(DashboardTrendResponse {
            sampled_at_ms: latest.sampled_at_ms,
            completeness: summary_completeness(
                &self.shared,
                latest.sampled_at_ms,
                series.completeness,
            ),
            points: series.points,
        })
    }

    fn disk_trend(
        &self,
        request: DashboardTrendRequest,
    ) -> Result<DashboardTrendResponse, DashboardQueryError> {
        let (reply, response) = mpsc::sync_channel(1);
        match self.writer.try_send(WriterCommand::Load(reply)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                return Err(DashboardQueryError::Unavailable);
            }
        }
        let report = response
            .recv_timeout(DISK_QUERY_TIMEOUT)
            .map_err(|_| DashboardQueryError::Unavailable)?
            .ok_or(DashboardQueryError::Unavailable)?;
        let writer_degraded = self
            .shared
            .writer_status
            .lock()
            .map(|status| !status.available || status.write_failures > 0)
            .unwrap_or(true);
        Ok(aggregate_disk_trend(
            &report.records,
            request.scope,
            wall_time_ms(),
            request.max_points,
            report.skipped_segments > 0 || writer_degraded,
        ))
    }
}

impl DashboardRuntime {
    pub(crate) fn service(&self) -> DashboardService {
        let writer = self.writer.as_ref().expect("dashboard writer available");
        DashboardService::new(Arc::clone(&self.shared), writer.sender.clone())
    }
}

fn page_start(sessions: &[SessionMetrics], cursor: u32) -> Result<usize, DashboardQueryError> {
    if cursor == 0 {
        return Ok(0);
    }
    sessions
        .binary_search_by_key(&cursor, |session| session.session_id)
        .map(|index| index + 1)
        .map_err(|_| DashboardQueryError::InvalidCursor)
}

fn summary_completeness(
    shared: &SharedDashboard,
    sampled_at_ms: u64,
    completeness: Completeness,
) -> Completeness {
    if completeness == Completeness::Unavailable {
        return completeness;
    }
    let running = shared
        .worker_status
        .lock()
        .map(|status| status.running)
        .unwrap_or(false);
    if !running
        || wall_time_ms().saturating_sub(sampled_at_ms) > SAMPLE_INTERVAL.as_millis() as u64 * 3
    {
        Completeness::Stale
    } else {
        completeness
    }
}

pub(crate) fn unavailable_summary() -> DashboardSummaryResponse {
    DashboardSummaryResponse {
        sampled_at_ms: 0,
        completeness: Completeness::Unavailable,
        daemon: DaemonMetrics {
            pid: std::process::id(),
            rates_available: false,
            cpu_milli_percent: 0,
            rss_kib: 0,
            read_bytes_per_sec: 0,
            write_bytes_per_sec: 0,
            session_count: 0,
            runtime_count: 0,
            active_writer_count: 0,
            readonly_client_count: 0,
        },
        sessions: Vec::new(),
        next_cursor: None,
    }
}

pub(crate) fn unavailable_trend() -> DashboardTrendResponse {
    DashboardTrendResponse {
        sampled_at_ms: 0,
        completeness: Completeness::Unavailable,
        points: Vec::new(),
    }
}

fn wall_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
