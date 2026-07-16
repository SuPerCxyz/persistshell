use std::collections::VecDeque;

use persist_ipc::{Completeness, TrendPoint, TrendScope};

use super::model::{DerivedSample, DerivedSession};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TrendSeries {
    pub completeness: Completeness,
    pub points: Vec<TrendPoint>,
}

pub(super) fn aggregate(
    frames: &VecDeque<Box<DerivedSample>>,
    scope: TrendScope,
    start_ms: u64,
    end_ms: u64,
    max_points: u16,
) -> TrendSeries {
    let width = end_ms
        .saturating_sub(start_ms)
        .div_ceil(u64::from(max_points))
        .max(1);
    let mut buckets = vec![None; max_points as usize];
    let mut completeness = Completeness::Complete;
    for sample in frames
        .iter()
        .map(Box::as_ref)
        .filter(|sample| sample.monotonic_ms > start_ms && sample.monotonic_ms <= end_ms)
    {
        let Some(metric) = metric_for(sample, scope) else {
            continue;
        };
        if sample.completeness != Completeness::Complete
            || !metric.complete
            || !metric.rates_available
        {
            completeness = Completeness::Partial;
        }
        let elapsed = sample
            .monotonic_ms
            .saturating_sub(start_ms)
            .saturating_sub(1);
        let index = (elapsed / width).min(u64::from(max_points - 1)) as usize;
        buckets[index]
            .get_or_insert_with(Accumulator::default)
            .add(sample.sampled_at_ms, metric);
    }
    let points = buckets
        .into_iter()
        .flatten()
        .map(Accumulator::finish)
        .collect::<Vec<_>>();
    if points.is_empty() {
        completeness = Completeness::Unavailable;
    }
    TrendSeries {
        completeness,
        points,
    }
}

impl TrendSeries {
    pub(super) fn unavailable() -> Self {
        Self {
            completeness: Completeness::Unavailable,
            points: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Metric {
    complete: bool,
    rates_available: bool,
    cpu_milli_percent: u32,
    rss_kib: u64,
    read_bytes_delta: u64,
    write_bytes_delta: u64,
    process_count: u32,
    session_count: u32,
    runtime_count: u32,
    active_writer_count: u32,
    readonly_client_count: u32,
}

fn metric_for(sample: &DerivedSample, scope: TrendScope) -> Option<Metric> {
    match scope {
        TrendScope::Daemon => Some(Metric {
            complete: true,
            rates_available: sample.daemon.metrics.rates_available,
            cpu_milli_percent: sample.daemon.metrics.cpu_milli_percent,
            rss_kib: sample.daemon.metrics.rss_kib,
            read_bytes_delta: sample.daemon.read_bytes_delta,
            write_bytes_delta: sample.daemon.write_bytes_delta,
            process_count: sample.daemon.process_count,
            session_count: sample.daemon.metrics.session_count,
            runtime_count: sample.daemon.metrics.runtime_count,
            active_writer_count: sample.daemon.metrics.active_writer_count,
            readonly_client_count: sample.daemon.metrics.readonly_client_count,
        }),
        TrendScope::Session(session_id) => session_metric(&sample.sessions, session_id),
    }
}

fn session_metric(sessions: &[DerivedSession], session_id: u32) -> Option<Metric> {
    sessions
        .iter()
        .find(|session| session.metrics.session_id == session_id)
        .map(|session| Metric {
            complete: session.metrics.collection_status == persist_ipc::CollectionStatus::Complete,
            rates_available: session.metrics.rates_available,
            cpu_milli_percent: session.metrics.cpu_milli_percent,
            rss_kib: session.metrics.rss_kib,
            read_bytes_delta: session.read_bytes_delta,
            write_bytes_delta: session.write_bytes_delta,
            process_count: session.metrics.process_count,
            session_count: 0,
            runtime_count: 0,
            active_writer_count: 0,
            readonly_client_count: 0,
        })
}

#[derive(Debug, Clone, Default)]
struct Accumulator {
    timestamp_ms: u64,
    cpu_sum: u128,
    cpu_count: u64,
    cpu_peak: u32,
    rss_sum: u128,
    rss_count: u64,
    rss_peak: u64,
    read_bytes: u128,
    write_bytes: u128,
    process_count_peak: u32,
    session_count: u32,
    runtime_count: u32,
    active_writer_count: u32,
    readonly_client_count: u32,
}

impl Accumulator {
    fn add(&mut self, timestamp_ms: u64, metric: Metric) {
        self.timestamp_ms = timestamp_ms;
        if metric.rates_available {
            self.cpu_sum += u128::from(metric.cpu_milli_percent);
            self.cpu_count += 1;
            self.cpu_peak = self.cpu_peak.max(metric.cpu_milli_percent);
            self.read_bytes = self
                .read_bytes
                .saturating_add(u128::from(metric.read_bytes_delta));
            self.write_bytes = self
                .write_bytes
                .saturating_add(u128::from(metric.write_bytes_delta));
        }
        self.rss_sum += u128::from(metric.rss_kib);
        self.rss_count += 1;
        self.rss_peak = self.rss_peak.max(metric.rss_kib);
        self.process_count_peak = self.process_count_peak.max(metric.process_count);
        self.session_count = self.session_count.max(metric.session_count);
        self.runtime_count = self.runtime_count.max(metric.runtime_count);
        self.active_writer_count = self.active_writer_count.max(metric.active_writer_count);
        self.readonly_client_count = self.readonly_client_count.max(metric.readonly_client_count);
    }

    fn finish(self) -> TrendPoint {
        TrendPoint {
            timestamp_ms: self.timestamp_ms,
            cpu_avg_milli_percent: average_u32(self.cpu_sum, self.cpu_count),
            cpu_peak_milli_percent: self.cpu_peak,
            rss_avg_kib: average_u64(self.rss_sum, self.rss_count),
            rss_peak_kib: self.rss_peak,
            read_bytes: saturating_u64(self.read_bytes),
            write_bytes: saturating_u64(self.write_bytes),
            process_count_peak: self.process_count_peak,
            session_count: self.session_count,
            runtime_count: self.runtime_count,
            active_writer_count: self.active_writer_count,
            readonly_client_count: self.readonly_client_count,
        }
    }
}

fn average_u32(sum: u128, count: u64) -> u32 {
    if count == 0 {
        0
    } else {
        (sum / u128::from(count)).min(u128::from(u32::MAX)) as u32
    }
}

fn average_u64(sum: u128, count: u64) -> u64 {
    if count == 0 {
        0
    } else {
        saturating_u64(sum / u128::from(count))
    }
}

fn saturating_u64(value: u128) -> u64 {
    value.min(u128::from(u64::MAX)) as u64
}
