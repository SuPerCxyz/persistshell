use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use persist_ipc::{
    Completeness, DashboardSummaryRequest, DashboardTrendRequest, TrendRange, TrendScope,
};

use super::format_tests::minute;
use super::history::BoundedHistory;
use super::history_tests::sample;
use super::ipc::*;
use super::ipc_disk::aggregate_disk_trend;
use super::storage::StorageLimits;
use super::worker::{SharedDashboard, WorkerStatus};
use super::writer::{spawn_writer, WriterCommand, WriterStatus};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn service_with_history(history: BoundedHistory) -> DashboardService {
    service_with_status(history, true)
}

fn service_with_status(history: BoundedHistory, running: bool) -> DashboardService {
    let (writer, _receiver) = mpsc::sync_channel(2);
    DashboardService::new(
        Arc::new(SharedDashboard {
            history: RwLock::new(history),
            worker_status: Mutex::new(WorkerStatus {
                running,
                ..WorkerStatus::default()
            }),
            writer_status: Arc::new(Mutex::new(WriterStatus::default())),
        }),
        writer,
    )
}

#[test]
fn empty_summary_and_invalid_cursor_are_explicit() {
    let service = service_with_history(BoundedHistory::new());
    let response = service
        .summary(DashboardSummaryRequest {
            cursor: 0,
            limit: 10,
        })
        .unwrap();
    assert_eq!(response.completeness, Completeness::Unavailable);
    assert!(response.sessions.is_empty());
    assert_eq!(
        service.summary(DashboardSummaryRequest {
            cursor: 1,
            limit: 10,
        }),
        Err(DashboardQueryError::InvalidCursor)
    );
    assert_eq!(
        service.summary(DashboardSummaryRequest {
            cursor: 0,
            limit: 0,
        }),
        Err(DashboardQueryError::InvalidRequest)
    );
    assert_eq!(
        service.trend(DashboardTrendRequest {
            scope: TrendScope::Daemon,
            range: TrendRange::Day,
            max_points: 0,
        }),
        Err(DashboardQueryError::InvalidRequest)
    );
}

#[test]
fn summary_pages_by_stable_session_id_cursor() {
    let mut history = BoundedHistory::new();
    let mut latest = sample(10_000, 100, 3);
    latest.sampled_at_ms = now_ms();
    history.push(latest);
    let service = service_with_history(history);

    let first = service
        .summary(DashboardSummaryRequest {
            cursor: 0,
            limit: 2,
        })
        .unwrap();
    assert_eq!(
        first
            .sessions
            .iter()
            .map(|item| item.session_id)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(first.next_cursor, Some(2));
    let second = service
        .summary(DashboardSummaryRequest {
            cursor: 2,
            limit: 2,
        })
        .unwrap();
    assert_eq!(second.sessions[0].session_id, 3);
    assert_eq!(second.next_cursor, None);
    assert_eq!(
        service.summary(DashboardSummaryRequest {
            cursor: 99,
            limit: 2,
        }),
        Err(DashboardQueryError::InvalidCursor)
    );
}

#[test]
fn memory_trend_bounds_points_and_unknown_session_is_unavailable() {
    let mut history = BoundedHistory::new();
    let wall = now_ms();
    for index in 1..=4 {
        let mut item = sample(index * 1_000, index as u32 * 100, 1);
        item.sampled_at_ms = wall + index;
        history.push(item);
    }
    let service = service_with_history(history);
    let daemon = service
        .trend(DashboardTrendRequest {
            scope: TrendScope::Daemon,
            range: TrendRange::FifteenMinutes,
            max_points: 2,
        })
        .unwrap();
    assert!(daemon.points.len() <= 2);
    let unknown = service
        .trend(DashboardTrendRequest {
            scope: TrendScope::Session(99),
            range: TrendRange::Hour,
            max_points: 10,
        })
        .unwrap();
    assert_eq!(unknown.completeness, Completeness::Unavailable);
    assert!(unknown.points.is_empty());
}

#[test]
fn stopped_worker_marks_existing_summary_and_trend_stale() {
    let mut history = BoundedHistory::new();
    let mut latest = sample(10_000, 100, 1);
    latest.sampled_at_ms = now_ms();
    history.push(latest);
    let service = service_with_status(history, false);
    let summary = service
        .summary(DashboardSummaryRequest {
            cursor: 0,
            limit: 10,
        })
        .unwrap();
    assert_eq!(summary.completeness, Completeness::Stale);
    let trend = service
        .trend(DashboardTrendRequest {
            scope: TrendScope::Daemon,
            range: TrendRange::Hour,
            max_points: 10,
        })
        .unwrap();
    assert_eq!(trend.completeness, Completeness::Stale);
}

#[test]
fn disk_trend_downsamples_and_propagates_partial_status() {
    let mut records = vec![minute(60_000), minute(120_000), minute(180_000)];
    records[1].completeness = Completeness::Partial;
    let response = aggregate_disk_trend(&records, TrendScope::Daemon, 180_000, 1, false);
    assert_eq!(response.points.len(), 1);
    assert_eq!(response.completeness, Completeness::Partial);
    assert_eq!(response.points[0].read_bytes, 312);

    let unknown = aggregate_disk_trend(&records, TrendScope::Session(99), 180_000, 240, false);
    assert_eq!(unknown.completeness, Completeness::Unavailable);
}

#[test]
fn full_writer_queue_makes_day_query_unavailable() {
    let (writer, _receiver) = mpsc::sync_channel(2);
    writer
        .try_send(WriterCommand::Append(minute(60_000)))
        .unwrap();
    writer
        .try_send(WriterCommand::Append(minute(120_000)))
        .unwrap();
    let service = DashboardService::new(
        Arc::new(SharedDashboard {
            history: RwLock::new(BoundedHistory::new()),
            worker_status: Mutex::new(WorkerStatus::default()),
            writer_status: Arc::new(Mutex::new(WriterStatus::default())),
        }),
        writer,
    );
    assert_eq!(
        service.trend(DashboardTrendRequest {
            scope: TrendScope::Daemon,
            range: TrendRange::Day,
            max_points: 10,
        }),
        Err(DashboardQueryError::Unavailable)
    );
}

#[test]
fn day_query_is_serialized_through_writer_storage() {
    let temp = tempfile::tempdir().unwrap();
    let writer = spawn_writer(temp.path().join("metrics"), StorageLimits::default());
    let service = DashboardService::new(
        Arc::new(SharedDashboard {
            history: RwLock::new(BoundedHistory::new()),
            worker_status: Mutex::new(WorkerStatus::default()),
            writer_status: Arc::clone(&writer.status),
        }),
        writer.sender.clone(),
    );
    let timestamp = now_ms() / 60_000 * 60_000;
    let mut record = minute(timestamp);
    record.completeness = Completeness::Complete;
    writer.sender.send(WriterCommand::Append(record)).unwrap();
    let response = service
        .trend(DashboardTrendRequest {
            scope: TrendScope::Daemon,
            range: TrendRange::Day,
            max_points: 10,
        })
        .unwrap();
    assert_eq!(response.points.len(), 1);
    assert_eq!(response.completeness, Completeness::Complete);

    drop(service);
    drop(writer.sender);
    writer
        .completion
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    writer.handle.join().unwrap();
}
