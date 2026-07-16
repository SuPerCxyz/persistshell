use std::collections::HashMap;

use persist_ipc::{CollectionStatus, Completeness, DaemonMetrics, SessionMetrics};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct RawCounters {
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub rss_kib: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub process_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RawDaemonSample {
    pub pid: u32,
    pub counters: RawCounters,
    pub session_count: u32,
    pub runtime_count: u32,
    pub active_writer_count: u32,
    pub readonly_client_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RawSessionSample {
    pub session_id: u32,
    pub counters: RawCounters,
    pub foreground_pid: Option<u32>,
    pub writer_active: bool,
    pub collection_status: CollectionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RawSample {
    pub sampled_at_ms: u64,
    pub monotonic_ms: u64,
    pub clock_ticks_per_second: u64,
    pub completeness: Completeness,
    pub daemon: RawDaemonSample,
    pub sessions: Vec<RawSessionSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DerivedDaemon {
    pub metrics: DaemonMetrics,
    pub process_count: u32,
    pub read_bytes_delta: u64,
    pub write_bytes_delta: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DerivedSession {
    pub metrics: SessionMetrics,
    pub read_bytes_delta: u64,
    pub write_bytes_delta: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DerivedSample {
    pub sampled_at_ms: u64,
    pub monotonic_ms: u64,
    pub completeness: Completeness,
    pub daemon: DerivedDaemon,
    pub sessions: Vec<DerivedSession>,
}

impl DerivedSample {
    pub(super) fn data_age_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.sampled_at_ms)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Rates {
    available: bool,
    cpu_milli_percent: u32,
    read_bytes_per_sec: u64,
    write_bytes_per_sec: u64,
    read_bytes_delta: u64,
    write_bytes_delta: u64,
}

pub(super) fn derive_sample(previous: Option<&RawSample>, current: &RawSample) -> DerivedSample {
    let elapsed_ms = previous
        .and_then(|sample| current.monotonic_ms.checked_sub(sample.monotonic_ms))
        .filter(|elapsed| *elapsed > 0);
    let previous_sessions = previous
        .map(|sample| {
            sample
                .sessions
                .iter()
                .map(|session| (session.session_id, session.counters))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let daemon_rates = previous
        .zip(elapsed_ms)
        .and_then(|(sample, elapsed)| {
            calculate_rates(
                sample.daemon.counters,
                current.daemon.counters,
                elapsed,
                current.clock_ticks_per_second,
            )
        })
        .unwrap_or_default();
    let sessions = current
        .sessions
        .iter()
        .map(|session| {
            let rates = previous_sessions
                .get(&session.session_id)
                .zip(elapsed_ms)
                .and_then(|(prior, elapsed)| {
                    calculate_rates(
                        *prior,
                        session.counters,
                        elapsed,
                        current.clock_ticks_per_second,
                    )
                })
                .unwrap_or_default();
            derived_session(*session, rates)
        })
        .collect();
    DerivedSample {
        sampled_at_ms: current.sampled_at_ms,
        monotonic_ms: current.monotonic_ms,
        completeness: current.completeness,
        daemon: derived_daemon(current.daemon, daemon_rates),
        sessions,
    }
}

fn derived_daemon(raw: RawDaemonSample, rates: Rates) -> DerivedDaemon {
    DerivedDaemon {
        metrics: DaemonMetrics {
            pid: raw.pid,
            rates_available: rates.available,
            cpu_milli_percent: rates.cpu_milli_percent,
            rss_kib: raw.counters.rss_kib,
            read_bytes_per_sec: rates.read_bytes_per_sec,
            write_bytes_per_sec: rates.write_bytes_per_sec,
            session_count: raw.session_count,
            runtime_count: raw.runtime_count,
            active_writer_count: raw.active_writer_count,
            readonly_client_count: raw.readonly_client_count,
        },
        process_count: raw.counters.process_count,
        read_bytes_delta: rates.read_bytes_delta,
        write_bytes_delta: rates.write_bytes_delta,
    }
}

fn derived_session(raw: RawSessionSample, rates: Rates) -> DerivedSession {
    DerivedSession {
        metrics: SessionMetrics {
            session_id: raw.session_id,
            process_count: raw.counters.process_count,
            rates_available: rates.available,
            cpu_milli_percent: rates.cpu_milli_percent,
            rss_kib: raw.counters.rss_kib,
            read_bytes_per_sec: rates.read_bytes_per_sec,
            write_bytes_per_sec: rates.write_bytes_per_sec,
            foreground_pid: raw.foreground_pid,
            writer_active: raw.writer_active,
            collection_status: raw.collection_status,
        },
        read_bytes_delta: rates.read_bytes_delta,
        write_bytes_delta: rates.write_bytes_delta,
    }
}

fn calculate_rates(
    previous: RawCounters,
    current: RawCounters,
    elapsed_ms: u64,
    ticks_per_second: u64,
) -> Option<Rates> {
    if elapsed_ms == 0 || ticks_per_second == 0 {
        return None;
    }
    let user_delta = current.user_ticks.checked_sub(previous.user_ticks)?;
    let system_delta = current.system_ticks.checked_sub(previous.system_ticks)?;
    let read_delta = current.read_bytes.checked_sub(previous.read_bytes)?;
    let write_delta = current.write_bytes.checked_sub(previous.write_bytes)?;
    let tick_delta = u128::from(user_delta) + u128::from(system_delta);
    let cpu_denominator = u128::from(ticks_per_second) * u128::from(elapsed_ms);
    let cpu = tick_delta.saturating_mul(100_000).saturating_mul(1_000) / cpu_denominator;
    Some(Rates {
        available: true,
        cpu_milli_percent: cpu.min(u128::from(u32::MAX)) as u32,
        read_bytes_per_sec: bytes_per_second(read_delta, elapsed_ms),
        write_bytes_per_sec: bytes_per_second(write_delta, elapsed_ms),
        read_bytes_delta: read_delta,
        write_bytes_delta: write_delta,
    })
}

fn bytes_per_second(bytes: u64, elapsed_ms: u64) -> u64 {
    (u128::from(bytes).saturating_mul(1_000) / u128::from(elapsed_ms)).min(u128::from(u64::MAX))
        as u64
}
