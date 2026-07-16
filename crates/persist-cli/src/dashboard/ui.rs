use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table, TableState};
use ratatui::Frame;

use super::app::{App, SortKey, View};

pub(super) fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    if area.height < 8 || area.width < 42 {
        render_compact(frame, app, area);
        return;
    }
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);
    render_header(frame, app, sections[0]);
    match app.view {
        View::Sessions => render_sessions(frame, app, sections[1]),
        View::Detail => render_detail(frame, app, sections[1]),
    }
    let help = match app.view {
        View::Sessions => "j/k move  Enter detail  s sort  q quit",
        View::Detail => "r range  Esc back  q quit",
    };
    frame.render_widget(
        Paragraph::new(help).style(Style::default().fg(Color::DarkGray)),
        sections[2],
    );
}

fn render_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let daemon = app.snapshot.daemon;
    let state = if app.connected {
        "connected"
    } else {
        "disconnected"
    };
    let color = if app.connected {
        Color::Green
    } else {
        Color::Red
    };
    let line = Line::from(vec![
        Span::styled(
            "PersistShell ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(state, Style::default().fg(color)),
        Span::raw(format!(
            "  pid {}  cpu {}  rss {}  sessions {}",
            daemon.pid,
            percent(daemon.cpu_milli_percent),
            kib(daemon.rss_kib),
            app.snapshot.sessions.len()
        )),
    ]);
    let detail = if app.status.is_empty() {
        format!(
            "sampled_at={}  status={:?}",
            app.snapshot.sampled_at_ms, app.snapshot.completeness
        )
    } else {
        app.status.clone()
    };
    frame.render_widget(
        Paragraph::new(vec![
            line,
            Line::from(detail).style(Style::default().fg(Color::DarkGray)),
        ])
        .block(Block::default().borders(Borders::BOTTOM)),
        area,
    );
}

fn render_sessions(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let wide = area.width >= 88;
    let header = if wide {
        Row::new(["ID", "CPU", "RSS", "READ/s", "WRITE/s", "PROC", "WRITER"])
    } else {
        Row::new(["ID", "CPU", "RSS", "PROC"])
    }
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    let rows = app.snapshot.sessions.iter().map(|session| {
        let base = vec![
            Cell::from(session.session_id.to_string()),
            Cell::from(percent(session.cpu_milli_percent)),
            Cell::from(kib(session.rss_kib)),
        ];
        if wide {
            Row::new(base.into_iter().chain([
                Cell::from(bytes(session.read_bytes_per_sec)),
                Cell::from(bytes(session.write_bytes_per_sec)),
                Cell::from(session.process_count.to_string()),
                Cell::from(if session.writer_active { "yes" } else { "no" }),
            ]))
        } else {
            Row::new(
                base.into_iter()
                    .chain([Cell::from(session.process_count.to_string())]),
            )
        }
    });
    let widths = if wide {
        vec![
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(7),
            Constraint::Min(6),
        ]
    } else {
        vec![
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Min(6),
        ]
    };
    let title = format!("Sessions  sort={}", sort_name(app.sort));
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().title(title).borders(Borders::ALL))
        .row_highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .highlight_symbol("> ");
    let mut state = TableState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(session) = app.selected_session() else {
        frame.render_widget(
            Paragraph::new("No session selected")
                .block(Block::default().title("Detail").borders(Borders::ALL)),
            area,
        );
        return;
    };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(3)])
        .split(area);
    let summary = format!(
        "Session {}  cpu {}  rss {}  io {}/s  processes {}\nrange={}  samples={}",
        session.session_id,
        percent(session.cpu_milli_percent),
        kib(session.rss_kib),
        bytes(
            session
                .read_bytes_per_sec
                .saturating_add(session.write_bytes_per_sec)
        ),
        session.process_count,
        range_name(app.range),
        app.trend.points.len()
    );
    frame.render_widget(
        Paragraph::new(summary).block(Block::default().borders(Borders::ALL)),
        sections[0],
    );
    let cpu = app
        .trend
        .points
        .iter()
        .map(|point| u64::from(point.cpu_avg_milli_percent))
        .collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .data(&cpu)
            .style(Style::default().fg(Color::Cyan))
            .block(Block::default().title("CPU trend").borders(Borders::ALL)),
        sections[1],
    );
}

fn render_compact(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let selected = app
        .selected_session()
        .map(|session| {
            format!(
                "session {} cpu {} rss {}",
                session.session_id,
                percent(session.cpu_milli_percent),
                kib(session.rss_kib)
            )
        })
        .unwrap_or_else(|| "no active sessions".to_owned());
    frame.render_widget(
        Paragraph::new(format!(
            "PersistShell {}\ndaemon cpu {} rss {}\n{}\nq quit",
            if app.connected {
                "connected"
            } else {
                "disconnected"
            },
            percent(app.snapshot.daemon.cpu_milli_percent),
            kib(app.snapshot.daemon.rss_kib),
            selected
        ))
        .block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn percent(value: u32) -> String {
    format!("{:.1}%", f64::from(value) / 1_000.0)
}

fn kib(value: u64) -> String {
    if value >= 1_048_576 {
        format!("{:.1} GiB", value as f64 / 1_048_576.0)
    } else if value >= 1_024 {
        format!("{:.1} MiB", value as f64 / 1_024.0)
    } else {
        format!("{value} KiB")
    }
}

fn bytes(value: u64) -> String {
    if value >= 1_048_576 {
        format!("{:.1}M", value as f64 / 1_048_576.0)
    } else if value >= 1_024 {
        format!("{:.1}K", value as f64 / 1_024.0)
    } else {
        value.to_string()
    }
}

fn sort_name(value: SortKey) -> &'static str {
    match value {
        SortKey::Cpu => "CPU",
        SortKey::Rss => "RSS",
        SortKey::Io => "I/O",
        SortKey::Processes => "PROC",
        SortKey::SessionId => "ID",
    }
}

fn range_name(value: persist_ipc::TrendRange) -> &'static str {
    match value {
        persist_ipc::TrendRange::FifteenMinutes => "15m",
        persist_ipc::TrendRange::Hour => "1h",
        persist_ipc::TrendRange::Day => "24h",
    }
}
