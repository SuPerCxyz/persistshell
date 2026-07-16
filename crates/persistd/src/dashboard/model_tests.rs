use persist_ipc::{CollectionStatus, Completeness};

use super::model::*;

fn counters(ticks: u64, rss: u64, io: u64) -> RawCounters {
    RawCounters {
        user_ticks: ticks,
        system_ticks: ticks,
        rss_kib: rss,
        read_bytes: io,
        write_bytes: io * 2,
        process_count: 2,
    }
}

fn sample(wall: u64, monotonic: u64, ticks: u64, io: u64) -> RawSample {
    RawSample {
        sampled_at_ms: wall,
        monotonic_ms: monotonic,
        clock_ticks_per_second: 100,
        completeness: Completeness::Complete,
        daemon: RawDaemonSample {
            pid: 10,
            counters: counters(ticks, 1_024, io),
            session_count: 1,
            runtime_count: 1,
            active_writer_count: 0,
            readonly_client_count: 0,
        },
        sessions: vec![RawSessionSample {
            session_id: 1,
            counters: counters(ticks, 512, io),
            foreground_pid: Some(20),
            writer_active: false,
            collection_status: CollectionStatus::Complete,
        }],
    }
}

#[test]
fn first_sample_has_values_but_no_rates() {
    let current = sample(1_000, 1_000, 10, 100);
    let derived = derive_sample(None, &current);
    assert!(!derived.daemon.metrics.rates_available);
    assert_eq!(derived.daemon.metrics.rss_kib, 1_024);
    assert!(!derived.sessions[0].metrics.rates_available);
}

#[test]
fn counter_deltas_produce_cpu_and_io_rates() {
    let previous = sample(1_000, 1_000, 10, 100);
    let current = sample(6_000, 6_000, 35, 600);
    let derived = derive_sample(Some(&previous), &current);
    assert!(derived.daemon.metrics.rates_available);
    assert_eq!(derived.daemon.metrics.cpu_milli_percent, 10_000);
    assert_eq!(derived.daemon.metrics.read_bytes_per_sec, 100);
    assert_eq!(derived.daemon.read_bytes_delta, 500);
    assert_eq!(derived.daemon.write_bytes_delta, 1_000);
}

#[test]
fn counter_or_monotonic_reset_invalidates_rates() {
    let previous = sample(2_000, 2_000, 20, 200);
    for current in [sample(3_000, 3_000, 10, 100), sample(3_000, 2_000, 30, 300)] {
        let derived = derive_sample(Some(&previous), &current);
        assert!(!derived.daemon.metrics.rates_available);
        assert_eq!(derived.daemon.metrics.cpu_milli_percent, 0);
        assert_eq!(derived.daemon.read_bytes_delta, 0);
    }
}

#[test]
fn wall_clock_rollback_does_not_break_valid_rates() {
    let previous = sample(5_000, 1_000, 10, 100);
    let current = sample(4_000, 6_000, 35, 600);
    let derived = derive_sample(Some(&previous), &current);
    assert!(derived.daemon.metrics.rates_available);
    assert_eq!(derived.sampled_at_ms, 4_000);
    assert_eq!(derived.data_age_ms(3_000), 0);
}

#[test]
fn rate_math_saturates_without_overflow() {
    let mut previous = sample(0, 1, 0, 0);
    previous.daemon.counters.user_ticks = 0;
    previous.daemon.counters.system_ticks = 0;
    let mut current = sample(1, 2, 0, 0);
    current.clock_ticks_per_second = 1;
    current.daemon.counters.user_ticks = u64::MAX;
    current.daemon.counters.system_ticks = u64::MAX;
    current.daemon.counters.read_bytes = u64::MAX;
    current.daemon.counters.write_bytes = u64::MAX;
    let derived = derive_sample(Some(&previous), &current);
    assert_eq!(derived.daemon.metrics.cpu_milli_percent, u32::MAX);
    assert_eq!(derived.daemon.metrics.read_bytes_per_sec, u64::MAX);
    assert_eq!(derived.daemon.metrics.write_bytes_per_sec, u64::MAX);
}
