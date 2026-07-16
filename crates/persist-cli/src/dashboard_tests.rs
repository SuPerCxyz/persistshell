use persist_ipc::{
    encode_summary_response, encode_trend_response, CollectionStatus, Completeness, DaemonMetrics,
    DashboardSummaryResponse, DashboardTrendResponse, Frame, MessageType, SessionMetrics,
};

use crate::dashboard::*;

fn daemon() -> DaemonMetrics {
    DaemonMetrics {
        pid: 10,
        rates_available: true,
        cpu_milli_percent: 100,
        rss_kib: 200,
        read_bytes_per_sec: 300,
        write_bytes_per_sec: 400,
        session_count: 3,
        runtime_count: 3,
        active_writer_count: 1,
        readonly_client_count: 0,
    }
}

fn session(id: u32) -> SessionMetrics {
    SessionMetrics {
        session_id: id,
        process_count: 1,
        rates_available: true,
        cpu_milli_percent: id,
        rss_kib: 100,
        read_bytes_per_sec: 0,
        write_bytes_per_sec: 0,
        foreground_pid: Some(100 + id),
        writer_active: false,
        collection_status: CollectionStatus::Complete,
    }
}

#[test]
fn summary_collector_advances_cursor_and_merges_pages() {
    let mut calls = 0;
    let snapshot = collect_summary_pages(|request| {
        calls += 1;
        match request.cursor {
            0 => Ok(DashboardSummaryResponse {
                sampled_at_ms: 1,
                completeness: Completeness::Complete,
                daemon: daemon(),
                sessions: vec![session(1), session(2)],
                next_cursor: Some(2),
            }),
            2 => Ok(DashboardSummaryResponse {
                sampled_at_ms: 2,
                completeness: Completeness::Partial,
                daemon: daemon(),
                sessions: vec![session(3)],
                next_cursor: None,
            }),
            _ => unreachable!(),
        }
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(snapshot.sessions.len(), 3);
    assert_eq!(snapshot.sampled_at_ms, 2);
    assert_eq!(snapshot.completeness, Completeness::Partial);
}

#[test]
fn summary_collector_rejects_duplicate_or_nonadvancing_cursor() {
    let duplicate = collect_summary_pages(|_| {
        Ok(DashboardSummaryResponse {
            sampled_at_ms: 1,
            completeness: Completeness::Complete,
            daemon: daemon(),
            sessions: vec![session(1), session(1)],
            next_cursor: None,
        })
    });
    assert!(duplicate.is_err());

    let bad_cursor = collect_summary_pages(|_| {
        Ok(DashboardSummaryResponse {
            sampled_at_ms: 1,
            completeness: Completeness::Complete,
            daemon: daemon(),
            sessions: vec![session(1)],
            next_cursor: Some(9),
        })
    });
    assert!(bad_cursor.is_err());
}

#[test]
fn frame_validation_rejects_wrong_type_and_request_id() {
    let summary = DashboardSummaryResponse {
        sampled_at_ms: 1,
        completeness: Completeness::Complete,
        daemon: daemon(),
        sessions: Vec::new(),
        next_cursor: None,
    };
    let valid = Frame {
        msg_type: MessageType::DashboardSummaryResp,
        flags: 0,
        request_id: 7,
        payload: encode_summary_response(&summary),
    };
    assert_eq!(parse_summary_frame(valid.clone(), 7).unwrap(), summary);
    assert!(parse_summary_frame(valid.clone(), 8).is_err());
    let mut wrong_type = valid;
    wrong_type.msg_type = MessageType::MetricsResp;
    assert!(parse_summary_frame(wrong_type, 7).is_err());

    let trend = DashboardTrendResponse {
        sampled_at_ms: 2,
        completeness: Completeness::Unavailable,
        points: Vec::new(),
    };
    let trend_frame = Frame {
        msg_type: MessageType::DashboardTrendResp,
        flags: 0,
        request_id: 9,
        payload: encode_trend_response(&trend),
    };
    assert_eq!(parse_trend_frame(trend_frame.clone(), 9).unwrap(), trend);
    assert!(parse_trend_frame(trend_frame, 10).is_err());
}

#[test]
fn refresh_policy_is_bounded_and_resets_after_success() {
    let mut policy = RefreshPolicy::default();
    assert_eq!(policy.next_delay(), std::time::Duration::from_secs(5));
    policy.record(false);
    assert_eq!(policy.next_delay(), std::time::Duration::from_millis(250));
    policy.record(false);
    assert_eq!(policy.next_delay(), std::time::Duration::from_millis(500));
    for _ in 0..20 {
        policy.record(false);
    }
    assert_eq!(policy.next_delay(), std::time::Duration::from_secs(5));
    policy.record(true);
    assert_eq!(policy.next_delay(), std::time::Duration::from_secs(5));
}
