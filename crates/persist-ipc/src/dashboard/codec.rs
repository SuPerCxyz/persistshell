use super::wire::*;
use super::*;

const SUMMARY_REQUEST_SIZE: usize = 6;
const TREND_REQUEST_SIZE: usize = 8;
const NO_VALUE: u32 = u32::MAX;

pub fn encode_summary_request(request: &DashboardSummaryRequest) -> Vec<u8> {
    if request.cursor == NO_VALUE || request.limit == 0 || request.limit > MAX_SUMMARY_PAGE {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(SUMMARY_REQUEST_SIZE);
    put_u32(&mut output, request.cursor);
    put_u16(&mut output, request.limit);
    output
}

pub fn decode_summary_request(payload: &[u8]) -> Option<DashboardSummaryRequest> {
    if payload.len() != SUMMARY_REQUEST_SIZE {
        return None;
    }
    let mut reader = Reader::new(payload);
    let request = DashboardSummaryRequest {
        cursor: reader.u32()?,
        limit: reader.u16()?,
    };
    (request.cursor != NO_VALUE && request.limit > 0 && request.limit <= MAX_SUMMARY_PAGE)
        .then_some(request)
}

pub fn encode_trend_request(request: &DashboardTrendRequest) -> Vec<u8> {
    if request.max_points == 0 || request.max_points > MAX_TREND_POINTS {
        return Vec::new();
    }
    let (scope, session_id) = match request.scope {
        TrendScope::Daemon => (0, 0),
        TrendScope::Session(0) => return Vec::new(),
        TrendScope::Session(session_id) => (1, session_id),
    };
    let range = match request.range {
        TrendRange::FifteenMinutes => 0,
        TrendRange::Hour => 1,
        TrendRange::Day => 2,
    };
    let mut output = Vec::with_capacity(TREND_REQUEST_SIZE);
    put_u8(&mut output, scope);
    put_u32(&mut output, session_id);
    put_u8(&mut output, range);
    put_u16(&mut output, request.max_points);
    output
}

pub fn decode_trend_request(payload: &[u8]) -> Option<DashboardTrendRequest> {
    if payload.len() != TREND_REQUEST_SIZE {
        return None;
    }
    let mut reader = Reader::new(payload);
    let scope = match (reader.u8()?, reader.u32()?) {
        (0, 0) => TrendScope::Daemon,
        (1, session_id) if session_id > 0 => TrendScope::Session(session_id),
        _ => return None,
    };
    let range = match reader.u8()? {
        0 => TrendRange::FifteenMinutes,
        1 => TrendRange::Hour,
        2 => TrendRange::Day,
        _ => return None,
    };
    let max_points = reader.u16()?;
    (max_points > 0 && max_points <= MAX_TREND_POINTS).then_some(DashboardTrendRequest {
        scope,
        range,
        max_points,
    })
}

pub fn encode_summary_response(response: &DashboardSummaryResponse) -> Vec<u8> {
    if response.sessions.len() > MAX_SUMMARY_PAGE as usize
        || response.next_cursor == Some(NO_VALUE)
        || response
            .sessions
            .iter()
            .any(|session| session.session_id == 0 || session.foreground_pid == Some(0))
    {
        return Vec::new();
    }
    let mut output = Vec::new();
    put_u64(&mut output, response.sampled_at_ms);
    put_u8(&mut output, encode_completeness(response.completeness));
    put_u32(&mut output, response.next_cursor.unwrap_or(NO_VALUE));
    encode_daemon(&mut output, response.daemon);
    put_u16(&mut output, response.sessions.len() as u16);
    for session in &response.sessions {
        encode_session(&mut output, *session);
    }
    output
}

pub fn decode_summary_response(payload: &[u8]) -> Option<DashboardSummaryResponse> {
    let mut reader = Reader::new(payload);
    let sampled_at_ms = reader.u64()?;
    let completeness = decode_completeness(reader.u8()?)?;
    let next_cursor = match reader.u32()? {
        NO_VALUE => None,
        cursor => Some(cursor),
    };
    let daemon = decode_daemon(&mut reader)?;
    let count = reader.u16()?;
    if count > MAX_SUMMARY_PAGE {
        return None;
    }
    let mut sessions = Vec::with_capacity(count as usize);
    for _ in 0..count {
        sessions.push(decode_session(&mut reader)?);
    }
    reader.finish().then_some(DashboardSummaryResponse {
        sampled_at_ms,
        completeness,
        daemon,
        sessions,
        next_cursor,
    })
}

fn encode_daemon(output: &mut Vec<u8>, daemon: DaemonMetrics) {
    put_u32(output, daemon.pid);
    put_u8(output, u8::from(daemon.rates_available));
    put_u32(output, daemon.cpu_milli_percent);
    put_u64(output, daemon.rss_kib);
    put_u64(output, daemon.read_bytes_per_sec);
    put_u64(output, daemon.write_bytes_per_sec);
    put_u32(output, daemon.session_count);
    put_u32(output, daemon.runtime_count);
    put_u32(output, daemon.active_writer_count);
    put_u32(output, daemon.readonly_client_count);
}

fn decode_daemon(reader: &mut Reader<'_>) -> Option<DaemonMetrics> {
    Some(DaemonMetrics {
        pid: reader.u32()?,
        rates_available: decode_bool(reader.u8()?)?,
        cpu_milli_percent: reader.u32()?,
        rss_kib: reader.u64()?,
        read_bytes_per_sec: reader.u64()?,
        write_bytes_per_sec: reader.u64()?,
        session_count: reader.u32()?,
        runtime_count: reader.u32()?,
        active_writer_count: reader.u32()?,
        readonly_client_count: reader.u32()?,
    })
}

fn encode_session(output: &mut Vec<u8>, session: SessionMetrics) {
    put_u32(output, session.session_id);
    put_u32(output, session.process_count);
    put_u8(output, u8::from(session.rates_available));
    put_u32(output, session.cpu_milli_percent);
    put_u64(output, session.rss_kib);
    put_u64(output, session.read_bytes_per_sec);
    put_u64(output, session.write_bytes_per_sec);
    put_u32(output, session.foreground_pid.unwrap_or(0));
    put_u8(output, u8::from(session.writer_active));
    put_u8(output, encode_collection_status(session.collection_status));
}

fn decode_session(reader: &mut Reader<'_>) -> Option<SessionMetrics> {
    let session_id = reader.u32()?;
    if session_id == 0 {
        return None;
    }
    let process_count = reader.u32()?;
    let rates_available = decode_bool(reader.u8()?)?;
    let cpu_milli_percent = reader.u32()?;
    let rss_kib = reader.u64()?;
    let read_bytes_per_sec = reader.u64()?;
    let write_bytes_per_sec = reader.u64()?;
    let foreground_pid = match reader.u32()? {
        0 => None,
        pid => Some(pid),
    };
    let writer_active = match reader.u8()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(SessionMetrics {
        session_id,
        process_count,
        rates_available,
        cpu_milli_percent,
        rss_kib,
        read_bytes_per_sec,
        write_bytes_per_sec,
        foreground_pid,
        writer_active,
        collection_status: decode_collection_status(reader.u8()?)?,
    })
}

fn decode_bool(value: u8) -> Option<bool> {
    match value {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

pub fn encode_trend_response(response: &DashboardTrendResponse) -> Vec<u8> {
    if response.points.len() > MAX_TREND_POINTS as usize {
        return Vec::new();
    }
    let mut output = Vec::new();
    put_u64(&mut output, response.sampled_at_ms);
    put_u8(&mut output, encode_completeness(response.completeness));
    put_u16(&mut output, response.points.len() as u16);
    for point in &response.points {
        encode_trend_point(&mut output, *point);
    }
    output
}

pub fn decode_trend_response(payload: &[u8]) -> Option<DashboardTrendResponse> {
    let mut reader = Reader::new(payload);
    let sampled_at_ms = reader.u64()?;
    let completeness = decode_completeness(reader.u8()?)?;
    let count = reader.u16()?;
    if count > MAX_TREND_POINTS {
        return None;
    }
    let mut points = Vec::with_capacity(count as usize);
    for _ in 0..count {
        points.push(decode_trend_point(&mut reader)?);
    }
    reader.finish().then_some(DashboardTrendResponse {
        sampled_at_ms,
        completeness,
        points,
    })
}

fn encode_trend_point(output: &mut Vec<u8>, point: TrendPoint) {
    put_u64(output, point.timestamp_ms);
    put_u32(output, point.cpu_avg_milli_percent);
    put_u32(output, point.cpu_peak_milli_percent);
    put_u64(output, point.rss_avg_kib);
    put_u64(output, point.rss_peak_kib);
    put_u64(output, point.read_bytes);
    put_u64(output, point.write_bytes);
    put_u32(output, point.process_count_peak);
    put_u32(output, point.session_count);
    put_u32(output, point.runtime_count);
    put_u32(output, point.active_writer_count);
    put_u32(output, point.readonly_client_count);
}

fn decode_trend_point(reader: &mut Reader<'_>) -> Option<TrendPoint> {
    Some(TrendPoint {
        timestamp_ms: reader.u64()?,
        cpu_avg_milli_percent: reader.u32()?,
        cpu_peak_milli_percent: reader.u32()?,
        rss_avg_kib: reader.u64()?,
        rss_peak_kib: reader.u64()?,
        read_bytes: reader.u64()?,
        write_bytes: reader.u64()?,
        process_count_peak: reader.u32()?,
        session_count: reader.u32()?,
        runtime_count: reader.u32()?,
        active_writer_count: reader.u32()?,
        readonly_client_count: reader.u32()?,
    })
}
