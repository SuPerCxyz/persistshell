use persist_ipc::{
    CollectionStatus, Completeness, DaemonMetrics, DashboardTrendResponse, SessionMetrics,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use super::app::{App, SortKey, View};
use super::{ui, DashboardSnapshot};

fn session(id: u32, cpu: u32, rss: u64) -> SessionMetrics {
    SessionMetrics {
        session_id: id,
        process_count: id,
        rates_available: true,
        cpu_milli_percent: cpu,
        rss_kib: rss,
        read_bytes_per_sec: u64::from(id) * 10,
        write_bytes_per_sec: u64::from(id) * 20,
        foreground_pid: Some(100 + id),
        writer_active: id == 2,
        collection_status: CollectionStatus::Complete,
    }
}

fn snapshot(sessions: Vec<SessionMetrics>) -> DashboardSnapshot {
    DashboardSnapshot {
        sampled_at_ms: 1,
        completeness: Completeness::Complete,
        daemon: DaemonMetrics {
            pid: 10,
            rates_available: true,
            cpu_milli_percent: 1_000,
            rss_kib: 2_048,
            read_bytes_per_sec: 0,
            write_bytes_per_sec: 0,
            session_count: sessions.len() as u32,
            runtime_count: sessions.len() as u32,
            active_writer_count: 1,
            readonly_client_count: 0,
        },
        sessions,
    }
}

fn app() -> App {
    App::new(
        snapshot(vec![session(1, 100, 300), session(2, 900, 100)]),
        DashboardTrendResponse {
            sampled_at_ms: 1,
            completeness: Completeness::Complete,
            points: Vec::new(),
        },
    )
}

#[test]
fn selection_sort_range_and_disappearing_session_are_stable() {
    let mut app = app();
    assert_eq!(app.snapshot.sessions[0].session_id, 2);
    app.move_selection(1);
    assert_eq!(app.selected_session().unwrap().session_id, 1);
    app.move_selection(99);
    assert_eq!(app.selected, 1);
    app.cycle_sort();
    assert_eq!(app.sort, SortKey::Rss);
    assert_eq!(app.selected_session().unwrap().session_id, 1);
    app.cycle_range();
    assert_eq!(app.range, persist_ipc::TrendRange::Hour);
    app.view = View::Detail;
    app.replace_snapshot(snapshot(vec![session(1, 100, 300)]));
    assert_eq!(app.selected, 0);
}

#[test]
fn regular_and_compact_rendering_do_not_overflow() {
    for (width, height) in [(100, 30), (60, 12), (40, 7)] {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = app();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.area.width, width);
        assert_eq!(buffer.area.height, height);
    }
}
