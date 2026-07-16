use std::collections::HashSet;
use std::time::Duration;

use persist_core::{Config, PersistError, Result};
use persist_ipc::{
    decode_summary_response, decode_trend_response, encode_summary_request, encode_trend_request,
    write_frame, Completeness, DaemonMetrics, DashboardSummaryRequest, DashboardSummaryResponse,
    DashboardTrendRequest, DashboardTrendResponse, Frame, MessageType, SessionMetrics,
    MAX_SUMMARY_PAGE,
};

const MAX_SUMMARY_SESSIONS: usize = 262_144;
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const MIN_RECONNECT_DELAY: Duration = Duration::from_millis(250);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(5);

mod app;
mod runtime;
mod terminal;
mod ui;

#[cfg(test)]
mod app_tests;

pub(crate) use runtime::run;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DashboardSnapshot {
    pub sampled_at_ms: u64,
    pub completeness: Completeness,
    pub daemon: DaemonMetrics,
    pub sessions: Vec<SessionMetrics>,
}

pub(crate) struct DashboardClient {
    socket: persist_ipc::ClientSocket,
    next_request_id: u32,
    timing: RefreshPolicy,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RefreshPolicy {
    failures: u8,
    next_delay: Duration,
}

impl Default for RefreshPolicy {
    fn default() -> Self {
        Self {
            failures: 0,
            next_delay: REFRESH_INTERVAL,
        }
    }
}

impl RefreshPolicy {
    pub(crate) fn record(&mut self, success: bool) {
        if success {
            self.failures = 0;
            self.next_delay = REFRESH_INTERVAL;
            return;
        }
        self.failures = self.failures.saturating_add(1);
        let multiplier = 1_u32 << u32::from(self.failures.saturating_sub(1).min(5));
        self.next_delay = (MIN_RECONNECT_DELAY * multiplier).min(MAX_RECONNECT_DELAY);
    }

    pub(crate) fn next_delay(&self) -> Duration {
        self.next_delay
    }
}

impl DashboardClient {
    pub(crate) fn connect(config: &Config) -> Result<Self> {
        let mut timing = RefreshPolicy::default();
        for attempt in 0..3 {
            match crate::session::connect_and_hello(config) {
                Ok(socket) => {
                    timing.record(true);
                    return Ok(Self {
                        socket,
                        next_request_id: 1,
                        timing,
                    });
                }
                Err(error) if attempt == 2 => return Err(error),
                Err(_) => {
                    timing.record(false);
                    std::thread::sleep(timing.next_delay());
                }
            }
        }
        unreachable!("bounded dashboard connection attempts")
    }

    pub(crate) fn summary(&mut self) -> Result<DashboardSnapshot> {
        let result = collect_summary_pages(|request| {
            let request_id = self.request_id();
            write_frame(
                self.socket.stream(),
                &Frame {
                    msg_type: MessageType::DashboardSummary,
                    flags: 0,
                    request_id,
                    payload: encode_summary_request(&request),
                },
            )?;
            let response = persist_ipc::read_frame(self.socket.stream())?;
            parse_summary_frame(response, request_id)
        });
        self.timing.record(result.is_ok());
        result
    }

    pub(crate) fn trend(
        &mut self,
        request: DashboardTrendRequest,
    ) -> Result<DashboardTrendResponse> {
        let result = (|| {
            let request_id = self.request_id();
            let payload = encode_trend_request(&request);
            if payload.is_empty() {
                return Err(protocol_error("invalid dashboard trend request"));
            }
            write_frame(
                self.socket.stream(),
                &Frame {
                    msg_type: MessageType::DashboardTrend,
                    flags: 0,
                    request_id,
                    payload,
                },
            )?;
            let response = persist_ipc::read_frame(self.socket.stream())?;
            parse_trend_frame(response, request_id)
        })();
        self.timing.record(result.is_ok());
        result
    }

    fn request_id(&mut self) -> u32 {
        let current = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        current
    }
}

pub(crate) fn collect_summary_pages(
    mut fetch: impl FnMut(DashboardSummaryRequest) -> Result<DashboardSummaryResponse>,
) -> Result<DashboardSnapshot> {
    let mut cursor = 0;
    let mut sessions = Vec::new();
    let mut seen = HashSet::new();
    let mut response = fetch(DashboardSummaryRequest {
        cursor,
        limit: MAX_SUMMARY_PAGE,
    })?;
    let mut sampled_at_ms = response.sampled_at_ms;
    let mut completeness = response.completeness;
    let mut daemon = response.daemon;
    loop {
        completeness = merge_completeness(completeness, response.completeness);
        let mut last_id = None;
        for session in response.sessions {
            if session.session_id <= cursor
                || last_id.is_some_and(|previous| session.session_id <= previous)
                || !seen.insert(session.session_id)
            {
                return Err(protocol_error("invalid dashboard summary pagination"));
            }
            last_id = Some(session.session_id);
            sessions.push(session);
            if sessions.len() > MAX_SUMMARY_SESSIONS {
                return Err(protocol_error("dashboard summary exceeds client limit"));
            }
        }
        match response.next_cursor {
            Some(next) if Some(next) == last_id && next > cursor => {
                cursor = next;
                response = fetch(DashboardSummaryRequest {
                    cursor,
                    limit: MAX_SUMMARY_PAGE,
                })?;
                sampled_at_ms = response.sampled_at_ms;
                daemon = response.daemon;
            }
            Some(_) => return Err(protocol_error("invalid dashboard summary cursor")),
            None => break,
        }
    }
    Ok(DashboardSnapshot {
        sampled_at_ms,
        completeness,
        daemon,
        sessions,
    })
}

fn merge_completeness(left: Completeness, right: Completeness) -> Completeness {
    use Completeness::{Complete, Partial, Stale, Unavailable};
    match (left, right) {
        (Unavailable, _) | (_, Unavailable) => Unavailable,
        (Stale, _) | (_, Stale) => Stale,
        (Partial, _) | (_, Partial) => Partial,
        _ => Complete,
    }
}

fn protocol_error(message: &'static str) -> PersistError {
    PersistError::invalid_argument(message)
}

pub(crate) fn parse_summary_frame(
    response: Frame,
    request_id: u32,
) -> Result<DashboardSummaryResponse> {
    if response.msg_type != MessageType::DashboardSummaryResp || response.request_id != request_id {
        return Err(protocol_error("unexpected dashboard summary response"));
    }
    decode_summary_response(&response.payload)
        .ok_or_else(|| protocol_error("invalid dashboard summary payload"))
}

pub(crate) fn parse_trend_frame(
    response: Frame,
    request_id: u32,
) -> Result<DashboardTrendResponse> {
    if response.msg_type != MessageType::DashboardTrendResp || response.request_id != request_id {
        return Err(protocol_error("unexpected dashboard trend response"));
    }
    decode_trend_response(&response.payload)
        .ok_or_else(|| protocol_error("invalid dashboard trend payload"))
}
