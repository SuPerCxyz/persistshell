use persist_ipc::{DashboardTrendResponse, SessionMetrics, TrendRange};

use super::DashboardSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Sessions,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SortKey {
    Cpu,
    Rss,
    Io,
    Processes,
    SessionId,
}

pub(super) struct App {
    pub snapshot: DashboardSnapshot,
    pub trend: DashboardTrendResponse,
    pub selected: usize,
    pub view: View,
    pub sort: SortKey,
    pub range: TrendRange,
    pub connected: bool,
    pub status: String,
}

impl App {
    pub(super) fn new(snapshot: DashboardSnapshot, trend: DashboardTrendResponse) -> Self {
        let mut app = Self {
            snapshot,
            trend,
            selected: 0,
            view: View::Sessions,
            sort: SortKey::Cpu,
            range: TrendRange::FifteenMinutes,
            connected: true,
            status: String::new(),
        };
        app.sort_sessions();
        app
    }

    pub(super) fn selected_session(&self) -> Option<&SessionMetrics> {
        self.snapshot.sessions.get(self.selected)
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        let len = self.snapshot.sessions.len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
    }

    pub(super) fn cycle_sort(&mut self) {
        let selected_id = self.selected_session().map(|session| session.session_id);
        self.sort = match self.sort {
            SortKey::Cpu => SortKey::Rss,
            SortKey::Rss => SortKey::Io,
            SortKey::Io => SortKey::Processes,
            SortKey::Processes => SortKey::SessionId,
            SortKey::SessionId => SortKey::Cpu,
        };
        self.sort_sessions();
        self.selected = selected_id
            .and_then(|id| {
                self.snapshot
                    .sessions
                    .iter()
                    .position(|session| session.session_id == id)
            })
            .unwrap_or(0);
    }

    pub(super) fn cycle_range(&mut self) {
        self.range = match self.range {
            TrendRange::FifteenMinutes => TrendRange::Hour,
            TrendRange::Hour => TrendRange::Day,
            TrendRange::Day => TrendRange::FifteenMinutes,
        };
    }

    pub(super) fn replace_snapshot(&mut self, snapshot: DashboardSnapshot) {
        let selected_id = self.selected_session().map(|session| session.session_id);
        self.snapshot = snapshot;
        self.sort_sessions();
        self.selected = selected_id
            .and_then(|id| {
                self.snapshot
                    .sessions
                    .iter()
                    .position(|session| session.session_id == id)
            })
            .unwrap_or(0);
    }

    fn sort_sessions(&mut self) {
        let sort = self.sort;
        self.snapshot.sessions.sort_by(|left, right| {
            let order = match sort {
                SortKey::Cpu => left.cpu_milli_percent.cmp(&right.cpu_milli_percent),
                SortKey::Rss => left.rss_kib.cmp(&right.rss_kib),
                SortKey::Io => io_total(left).cmp(&io_total(right)),
                SortKey::Processes => left.process_count.cmp(&right.process_count),
                SortKey::SessionId => right.session_id.cmp(&left.session_id),
            };
            order
                .reverse()
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        if self.selected >= self.snapshot.sessions.len() {
            self.selected = self.snapshot.sessions.len().saturating_sub(1);
        }
    }
}

fn io_total(session: &SessionMetrics) -> u64 {
    session
        .read_bytes_per_sec
        .saturating_add(session.write_bytes_per_sec)
}
