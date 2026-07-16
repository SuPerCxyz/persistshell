use persist_ipc::{Completeness, DashboardTrendResponse, TrendPoint, TrendScope};

use super::format::MinuteRecord;

const DAY_MS: u64 = 24 * 60 * 60 * 1_000;

pub(super) fn aggregate_disk_trend(
    records: &[MinuteRecord],
    scope: TrendScope,
    now_ms: u64,
    max_points: u16,
    skipped_segments: bool,
) -> DashboardTrendResponse {
    let end_ms = records
        .last()
        .map(|record| now_ms.max(record.timestamp_ms))
        .unwrap_or(now_ms);
    let start_ms = end_ms.saturating_sub(DAY_MS);
    let width = DAY_MS.div_ceil(u64::from(max_points)).max(1);
    let mut buckets = vec![None; usize::from(max_points)];
    let mut completeness = if skipped_segments {
        Completeness::Partial
    } else {
        Completeness::Complete
    };
    for record in records
        .iter()
        .filter(|record| record.timestamp_ms > start_ms && record.timestamp_ms <= end_ms)
    {
        let Some((point, point_completeness)) = select_point(record, scope) else {
            continue;
        };
        if record.completeness != Completeness::Complete
            || point_completeness != Completeness::Complete
        {
            completeness = Completeness::Partial;
        }
        let index = record
            .timestamp_ms
            .saturating_sub(start_ms)
            .saturating_sub(1)
            / width;
        buckets[index.min(u64::from(max_points - 1)) as usize]
            .get_or_insert_with(Accumulator::default)
            .add(point);
    }
    let points = buckets
        .into_iter()
        .flatten()
        .map(Accumulator::finish)
        .collect::<Vec<_>>();
    if points.is_empty() {
        completeness = Completeness::Unavailable;
    }
    DashboardTrendResponse {
        sampled_at_ms: records.last().map_or(0, |record| record.timestamp_ms),
        completeness,
        points,
    }
}

fn select_point(record: &MinuteRecord, scope: TrendScope) -> Option<(TrendPoint, Completeness)> {
    match scope {
        TrendScope::Daemon => Some((record.daemon, record.completeness)),
        TrendScope::Session(session_id) => record
            .sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .map(|session| (session.point, session.completeness)),
    }
}

#[derive(Clone, Default)]
struct Accumulator {
    timestamp_ms: u64,
    cpu_sum: u128,
    rss_sum: u128,
    count: u64,
    cpu_peak: u32,
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
    fn add(&mut self, point: TrendPoint) {
        self.timestamp_ms = point.timestamp_ms;
        self.cpu_sum = self
            .cpu_sum
            .saturating_add(u128::from(point.cpu_avg_milli_percent));
        self.rss_sum = self.rss_sum.saturating_add(u128::from(point.rss_avg_kib));
        self.count = self.count.saturating_add(1);
        self.cpu_peak = self.cpu_peak.max(point.cpu_peak_milli_percent);
        self.rss_peak = self.rss_peak.max(point.rss_peak_kib);
        self.read_bytes = self.read_bytes.saturating_add(u128::from(point.read_bytes));
        self.write_bytes = self
            .write_bytes
            .saturating_add(u128::from(point.write_bytes));
        self.process_count_peak = self.process_count_peak.max(point.process_count_peak);
        self.session_count = self.session_count.max(point.session_count);
        self.runtime_count = self.runtime_count.max(point.runtime_count);
        self.active_writer_count = self.active_writer_count.max(point.active_writer_count);
        self.readonly_client_count = self.readonly_client_count.max(point.readonly_client_count);
    }

    fn finish(self) -> TrendPoint {
        TrendPoint {
            timestamp_ms: self.timestamp_ms,
            cpu_avg_milli_percent: average_u32(self.cpu_sum, self.count),
            cpu_peak_milli_percent: self.cpu_peak,
            rss_avg_kib: average_u64(self.rss_sum, self.count),
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
