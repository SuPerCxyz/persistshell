mod codec;
mod wire;

pub use codec::*;

pub const MAX_SUMMARY_PAGE: u16 = 128;
pub const MAX_TREND_POINTS: u16 = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completeness {
    Complete,
    Partial,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionStatus {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardSummaryRequest {
    pub cursor: u32,
    pub limit: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaemonMetrics {
    pub pid: u32,
    pub cpu_milli_percent: u32,
    pub rss_kib: u64,
    pub read_bytes_per_sec: u64,
    pub write_bytes_per_sec: u64,
    pub session_count: u32,
    pub runtime_count: u32,
    pub active_writer_count: u32,
    pub readonly_client_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionMetrics {
    pub session_id: u32,
    pub process_count: u32,
    pub cpu_milli_percent: u32,
    pub rss_kib: u64,
    pub read_bytes_per_sec: u64,
    pub write_bytes_per_sec: u64,
    pub foreground_pid: Option<u32>,
    pub writer_active: bool,
    pub collection_status: CollectionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardSummaryResponse {
    pub sampled_at_ms: u64,
    pub completeness: Completeness,
    pub daemon: DaemonMetrics,
    pub sessions: Vec<SessionMetrics>,
    pub next_cursor: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendScope {
    Daemon,
    Session(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendRange {
    FifteenMinutes,
    Hour,
    Day,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardTrendRequest {
    pub scope: TrendScope,
    pub range: TrendRange,
    pub max_points: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrendPoint {
    pub timestamp_ms: u64,
    pub cpu_avg_milli_percent: u32,
    pub cpu_peak_milli_percent: u32,
    pub rss_avg_kib: u64,
    pub rss_peak_kib: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub process_count_peak: u32,
    pub session_count: u32,
    pub runtime_count: u32,
    pub active_writer_count: u32,
    pub readonly_client_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardTrendResponse {
    pub sampled_at_ms: u64,
    pub completeness: Completeness,
    pub points: Vec<TrendPoint>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_CONTROL_FRAME;

    #[test]
    fn summary_request_round_trip_and_bounds() {
        let request = DashboardSummaryRequest {
            cursor: 42,
            limit: 128,
        };
        assert_eq!(
            decode_summary_request(&encode_summary_request(&request)),
            Some(request)
        );
        assert!(decode_summary_request(&[0; 5]).is_none());
        assert!(encode_summary_request(&DashboardSummaryRequest {
            cursor: 0,
            limit: 129,
        })
        .is_empty());
        assert!(encode_summary_request(&DashboardSummaryRequest {
            cursor: u32::MAX,
            limit: 1,
        })
        .is_empty());
        let mut trailing = encode_summary_request(&request);
        trailing.push(0);
        assert!(decode_summary_request(&trailing).is_none());
    }

    #[test]
    fn summary_response_round_trip() {
        let response = DashboardSummaryResponse {
            sampled_at_ms: 123_456,
            completeness: Completeness::Partial,
            daemon: DaemonMetrics {
                pid: 7,
                cpu_milli_percent: 12_345,
                rss_kib: 4_096,
                read_bytes_per_sec: 100,
                write_bytes_per_sec: 200,
                session_count: 2,
                runtime_count: 1,
                active_writer_count: 1,
                readonly_client_count: 0,
            },
            sessions: vec![SessionMetrics {
                session_id: 9,
                process_count: 3,
                cpu_milli_percent: 250_000,
                rss_kib: 8_192,
                read_bytes_per_sec: 300,
                write_bytes_per_sec: 400,
                foreground_pid: Some(99),
                writer_active: true,
                collection_status: CollectionStatus::Complete,
            }],
            next_cursor: Some(43),
        };
        let encoded = encode_summary_response(&response);
        assert_eq!(decode_summary_response(&encoded), Some(response));
        assert!(decode_summary_response(&encoded[..encoded.len() - 1]).is_none());
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode_summary_response(&trailing).is_none());
        let mut invalid_status = encoded;
        *invalid_status.last_mut().expect("session status") = 9;
        assert!(decode_summary_response(&invalid_status).is_none());
    }

    #[test]
    fn trend_request_round_trip_and_limits() {
        let request = DashboardTrendRequest {
            scope: TrendScope::Session(11),
            range: TrendRange::Hour,
            max_points: 240,
        };
        assert_eq!(
            decode_trend_request(&encode_trend_request(&request)),
            Some(request)
        );
        for max_points in [0, 241] {
            assert!(encode_trend_request(&DashboardTrendRequest {
                max_points,
                ..request
            })
            .is_empty());
        }
    }

    #[test]
    fn maximum_trend_response_fits_control_frame() {
        let point = TrendPoint {
            timestamp_ms: 1,
            cpu_avg_milli_percent: 2,
            cpu_peak_milli_percent: 3,
            rss_avg_kib: 4,
            rss_peak_kib: 5,
            read_bytes: 6,
            write_bytes: 7,
            process_count_peak: 8,
            session_count: 9,
            runtime_count: 10,
            active_writer_count: 11,
            readonly_client_count: 12,
        };
        let response = DashboardTrendResponse {
            sampled_at_ms: 13,
            completeness: Completeness::Complete,
            points: vec![point; 240],
        };
        let encoded = encode_trend_response(&response);
        assert!(encoded.len() < MAX_CONTROL_FRAME);
        assert_eq!(decode_trend_response(&encoded), Some(response));
    }

    #[test]
    fn maximum_summary_response_fits_control_frame() {
        let session = SessionMetrics {
            session_id: 1,
            process_count: 2,
            cpu_milli_percent: 3,
            rss_kib: 4,
            read_bytes_per_sec: 5,
            write_bytes_per_sec: 6,
            foreground_pid: None,
            writer_active: false,
            collection_status: CollectionStatus::Unavailable,
        };
        let response = DashboardSummaryResponse {
            sampled_at_ms: 1,
            completeness: Completeness::Stale,
            daemon: DaemonMetrics {
                pid: 1,
                cpu_milli_percent: 2,
                rss_kib: 3,
                read_bytes_per_sec: 4,
                write_bytes_per_sec: 5,
                session_count: 128,
                runtime_count: 128,
                active_writer_count: 0,
                readonly_client_count: 0,
            },
            sessions: vec![session; MAX_SUMMARY_PAGE as usize],
            next_cursor: None,
        };
        let encoded = encode_summary_response(&response);
        assert!(encoded.len() < MAX_CONTROL_FRAME);
        assert_eq!(decode_summary_response(&encoded), Some(response));
    }

    #[test]
    fn decoders_reject_unknown_enums_and_trailing_data() {
        let request = DashboardTrendRequest {
            scope: TrendScope::Daemon,
            range: TrendRange::Day,
            max_points: 1,
        };
        let mut encoded = encode_trend_request(&request);
        encoded[0] = 9;
        assert!(decode_trend_request(&encoded).is_none());
        let mut encoded = encode_trend_request(&request);
        encoded[5] = 9;
        assert!(decode_trend_request(&encoded).is_none());
        let mut encoded = encode_trend_request(&request);
        encoded.push(0);
        assert!(decode_trend_request(&encoded).is_none());

        let response = DashboardTrendResponse {
            sampled_at_ms: 1,
            completeness: Completeness::Complete,
            points: Vec::new(),
        };
        let mut encoded = encode_trend_response(&response);
        encoded[8] = 9;
        assert!(decode_trend_response(&encoded).is_none());
        let mut encoded = encode_trend_response(&response);
        encoded.push(0);
        assert!(decode_trend_response(&encoded).is_none());
    }
}
