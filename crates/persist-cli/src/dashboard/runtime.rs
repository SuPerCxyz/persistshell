use std::io::Write;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use persist_core::{Config, PersistError, Result};
use persist_ipc::{DashboardTrendRequest, TrendScope, MAX_TREND_POINTS};

use super::app::{App, View};
use super::terminal::TerminalGuard;
use super::{ui, DashboardClient};

const REDRAW_INTERVAL: Duration = Duration::from_millis(250);
const DATA_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) fn run<W: Write>(config: &Config, _output: &mut W, interactive: bool) -> Result<()> {
    if !interactive {
        return Err(PersistError::invalid_argument(
            "persist top requires an interactive terminal",
        ));
    }
    let mut client = DashboardClient::connect(config)?;
    let snapshot = client.summary()?;
    let trend = client.trend(DashboardTrendRequest {
        scope: TrendScope::Daemon,
        range: persist_ipc::TrendRange::FifteenMinutes,
        max_points: MAX_TREND_POINTS,
    })?;
    let mut app = App::new(snapshot, trend);
    let mut terminal = TerminalGuard::enter()?;
    let mut next_refresh = Instant::now() + DATA_INTERVAL;

    loop {
        terminal.draw(|frame| ui::render(frame, &app))?;
        let timeout = next_refresh
            .saturating_duration_since(Instant::now())
            .min(REDRAW_INTERVAL);
        if event::poll(timeout).map_err(event_error)? {
            if let Event::Key(key) = event::read().map_err(event_error)? {
                if handle_key(key, &mut app, &mut client)? {
                    return Ok(());
                }
            }
        }
        if Instant::now() >= next_refresh {
            refresh(config, &mut client, &mut app);
            next_refresh = Instant::now() + DATA_INTERVAL;
        }
    }
}

fn handle_key(key: KeyEvent, app: &mut App, client: &mut DashboardClient) -> Result<bool> {
    if key.kind != KeyEventKind::Press {
        return Ok(false);
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }
    match (app.view, key.code) {
        (_, KeyCode::Char('q')) => return Ok(true),
        (View::Sessions, KeyCode::Down | KeyCode::Char('j')) => app.move_selection(1),
        (View::Sessions, KeyCode::Up | KeyCode::Char('k')) => app.move_selection(-1),
        (View::Sessions, KeyCode::Char('s')) => app.cycle_sort(),
        (View::Sessions, KeyCode::Enter) if app.selected_session().is_some() => {
            app.view = View::Detail;
            if let Err(error) = update_trend(client, app) {
                mark_disconnected(app, &error);
            }
        }
        (View::Detail, KeyCode::Esc) => app.view = View::Sessions,
        (View::Detail, KeyCode::Char('r')) => {
            app.cycle_range();
            if let Err(error) = update_trend(client, app) {
                mark_disconnected(app, &error);
            }
        }
        _ => {}
    }
    Ok(false)
}

fn refresh(config: &Config, client: &mut DashboardClient, app: &mut App) {
    match client.summary() {
        Ok(snapshot) => {
            app.replace_snapshot(snapshot);
            app.connected = true;
            app.status.clear();
            if app.view == View::Detail {
                if let Err(error) = update_trend(client, app) {
                    mark_disconnected(app, &error);
                }
            }
        }
        Err(error) => {
            mark_disconnected(app, &error);
            if let Ok(mut replacement) = DashboardClient::connect(config) {
                if let Ok(snapshot) = replacement.summary() {
                    *client = replacement;
                    app.replace_snapshot(snapshot);
                    app.connected = true;
                    app.status.clear();
                }
            }
        }
    }
}

fn update_trend(client: &mut DashboardClient, app: &mut App) -> Result<()> {
    let scope = app
        .selected_session()
        .map(|session| TrendScope::Session(session.session_id))
        .unwrap_or(TrendScope::Daemon);
    app.trend = client.trend(DashboardTrendRequest {
        scope,
        range: app.range,
        max_points: MAX_TREND_POINTS,
    })?;
    Ok(())
}

fn mark_disconnected(app: &mut App, error: &PersistError) {
    app.connected = false;
    app.status = error.to_string();
}

fn event_error(source: std::io::Error) -> PersistError {
    PersistError::Io {
        operation: "read dashboard event",
        source,
    }
}
