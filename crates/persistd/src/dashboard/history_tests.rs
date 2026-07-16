use persist_ipc::{CollectionStatus, Completeness, DaemonMetrics, SessionMetrics, TrendScope};

use super::history::*;
use super::model::*;

pub(super) fn sample(monotonic_ms: u64, cpu: u32, session_count: usize) -> DerivedSample {
    let rates = DaemonMetrics {
        pid: 10,
        rates_available: true,
        cpu_milli_percent: cpu,
        rss_kib: 1_000 + monotonic_ms,
        read_bytes_per_sec: 10,
        write_bytes_per_sec: 20,
        session_count: session_count as u32,
        runtime_count: session_count as u32,
        active_writer_count: 0,
        readonly_client_count: 0,
    };
    let sessions = (1..=session_count)
        .map(|id| DerivedSession {
            metrics: SessionMetrics {
                session_id: id as u32,
                process_count: 2,
                rates_available: true,
                cpu_milli_percent: cpu,
                rss_kib: 500,
                read_bytes_per_sec: 10,
                write_bytes_per_sec: 20,
                foreground_pid: None,
                writer_active: false,
                collection_status: CollectionStatus::Complete,
            },
            read_bytes_delta: 10,
            write_bytes_delta: 20,
        })
        .collect();
    DerivedSample {
        sampled_at_ms: monotonic_ms,
        monotonic_ms,
        completeness: Completeness::Complete,
        daemon: DerivedDaemon {
            metrics: rates,
            process_count: session_count as u32 * 2,
            read_bytes_delta: 10,
            write_bytes_delta: 20,
        },
        sessions,
    }
}

#[test]
fn history_evicts_old_time_slices_and_keeps_latest() {
    let mut history = BoundedHistory::with_limits(1_400, 60_000, 4);
    for timestamp in [1_000, 2_000, 3_000, 4_000] {
        history.push(sample(timestamp, timestamp as u32, 8));
    }
    assert!(history.len() < 4);
    assert!(history.memory_bytes() <= 1_400);
    assert_eq!(history.latest().unwrap().monotonic_ms, 4_000);
    assert!(history.oldest_monotonic_ms().unwrap() > 1_000);
}

#[test]
fn production_history_starts_within_memory_limit() {
    let history = BoundedHistory::new();
    assert_eq!(history.len(), 0);
    assert!(history.memory_bytes() <= MEMORY_LIMIT_BYTES);
}

#[test]
fn oversized_latest_frame_is_truncated_and_retained() {
    let mut history = BoundedHistory::with_limits(800, 60_000, 2);
    history.push(sample(1_000, 1, 100));
    let latest = history.latest().unwrap();
    assert!(latest.sessions.len() < 100);
    assert_eq!(latest.completeness, Completeness::Partial);
    assert!(history.memory_bytes() <= 800);
}

#[test]
fn history_enforces_age_and_frame_limits() {
    let mut history = BoundedHistory::with_limits(64 * 1024, 10_000, 2);
    for timestamp in [1_000, 6_000, 12_000] {
        history.push(sample(timestamp, 1, 1));
    }
    assert_eq!(history.len(), 2);
    assert_eq!(history.oldest_monotonic_ms(), Some(6_000));
}

#[test]
fn downsample_uses_bounded_time_buckets() {
    let mut history = BoundedHistory::with_limits(64 * 1024, 60_000, 8);
    for timestamp in [1_000, 2_000, 3_000, 4_000] {
        history.push(sample(timestamp, timestamp as u32, 1));
    }
    let series = history.trend(TrendScope::Daemon, 4_000, 4_000, 2);
    assert_eq!(series.points.len(), 2);
    assert_eq!(series.points[0].cpu_avg_milli_percent, 1_500);
    assert_eq!(series.points[1].cpu_avg_milli_percent, 3_500);
    assert_eq!(series.points[0].read_bytes, 20);
}

#[test]
fn minute_aggregate_and_unknown_session_are_explicit() {
    let mut history = BoundedHistory::with_limits(64 * 1024, 120_000, 8);
    history.push(sample(10_000, 1_000, 1));
    history.push(sample(50_000, 3_000, 1));
    let point = history.minute(TrendScope::Session(1), 0).unwrap();
    assert_eq!(point.cpu_avg_milli_percent, 2_000);
    assert_eq!(point.read_bytes, 20);
    let missing = history.trend(TrendScope::Session(99), 60_000, 60_000, 4);
    assert!(missing.points.is_empty());
    assert_eq!(missing.completeness, Completeness::Unavailable);
}

#[test]
fn minute_aggregate_uses_wall_clock_and_lists_observed_sessions() {
    let mut history = BoundedHistory::with_limits(64 * 1024, 120_000, 8);
    let mut first = sample(5_000, 1_000, 1);
    first.sampled_at_ms = 125_000;
    let mut second = sample(10_000, 3_000, 2);
    second.sampled_at_ms = 150_000;
    history.push(first);
    history.push(second);

    let point = history.minute(TrendScope::Daemon, 120_000).unwrap();
    assert_eq!(point.cpu_avg_milli_percent, 2_000);
    assert_eq!(history.session_ids_wall(120_000, 180_000), vec![1, 2]);
    assert!(history.minute(TrendScope::Daemon, 0).is_none());
}

#[test]
fn unavailable_rates_keep_rss_without_fabricating_io() {
    let mut history = BoundedHistory::with_limits(64 * 1024, 60_000, 4);
    let mut first = sample(1_000, 0, 1);
    first.daemon.metrics.rates_available = false;
    first.daemon.read_bytes_delta = 99;
    history.push(first);
    let series = history.trend(TrendScope::Daemon, 1_000, 1_000, 1);
    assert_eq!(series.completeness, Completeness::Partial);
    assert_eq!(series.points[0].rss_avg_kib, 2_000);
    assert_eq!(series.points[0].read_bytes, 0);
}
