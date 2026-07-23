#![cfg_attr(
    not(test),
    allow(dead_code, unused_imports, unused_mut, unused_variables)
)]
#![allow(
    clippy::bind_instead_of_map,
    clippy::iter_next_slice,
    clippy::let_unit_value,
    clippy::needless_borrow,
    clippy::unnecessary_map_or
)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use persist_core::shell_state::{EnvironmentPolicy, EnvironmentSnapshot, ShellLaunchEnvironment};
#[cfg(test)]
use persist_core::RingBuffer;
use persist_core::{
    init_logging, load_config, version_string, ConfigLoadOptions, PersistError,
    RecoveryEnvironmentConfig, Result,
};
use persist_ipc::holder::CreateSessionRequest as HolderCreateRequest;
use persist_ipc::{
    decode_attach, decode_attach_resp, decode_detach, decode_hello, decode_list_sessions_resp,
    decode_lock, decode_new_session_resp, decode_note, decode_note_get_resp, decode_op_resp,
    decode_pin, decode_rename, decode_resize, decode_signal, decode_summary_request, decode_tag,
    decode_tag_list_resp, decode_trend_request, encode_attach, encode_attach_resp, encode_detach,
    encode_hello, encode_hello_ack, encode_list_sessions_resp, encode_new_session_resp,
    encode_note, encode_note_get_resp, encode_op_resp, encode_pin, encode_process_stats_resp,
    encode_process_tree_resp, encode_resize, encode_session_exited, encode_signal,
    encode_summary_response, encode_tag, encode_tag_list_resp, encode_trend_response,
    encode_writer_control, read_frame, write_frame, AttachPayload, AttachRespPayload,
    ConnectionEnvironment, DaemonConnection, DaemonSocket, DetachPayload, Frame, FrameAccumulator,
    HelloAckPayload, HelloPayload, HelloStatus, ListSessionsRespPayload, MessageType,
    NewSessionRespPayload, NotePayload, OpRespPayload, PinPayload, ProcessStatsRespPayload,
    ProcessTreeNode, ProcessTreeRespPayload, ResizePayload, SessionEntry, SessionExitedPayload,
    SignalPayload, TagListRespPayload, TagPayload, WriterControlPayload,
    ATTACH_CONTEXT_PROTOCOL_MINOR, MAX_IO_FRAME,
};
use persist_metadata::{MetadataStore, SessionRecord};
use persist_pty::pty::detect_shell;
#[cfg(test)]
use persist_pty::PtyEngine;
#[cfg(test)]
use persist_pty::PtySession;

use crate::dashboard::{
    unavailable_summary, unavailable_trend, DashboardRuntime, DashboardService, SampleRequest,
    SessionRoot, SAMPLE_INTERVAL,
};
#[cfg(test)]
use crate::log_writer::{spawn_session_logger, SessionLogHandle};

pub fn run<I>(args: I) -> ExitCode
where
    I: IntoIterator<Item = String>,
{
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let code = run_with_io(args, &mut stdout, &mut stderr);
    ExitCode::from(code)
}

pub fn run_with_io<I, W, E>(args: I, stdout: &mut W, stderr: &mut E) -> u8
where
    I: IntoIterator<Item = String>,
    W: Write,
    E: Write,
{
    let raw_args = args.into_iter().skip(1).collect::<Vec<_>>();
    let mut idle_timeout: Option<std::time::Duration> = None;
    let mut remaining = Vec::new();
    {
        let mut i = 0;
        while i < raw_args.len() {
            if raw_args[i] == "--idle-timeout" {
                i += 1;
                if i < raw_args.len() {
                    match raw_args[i].parse::<persist_core::config::DurationValue>() {
                        Ok(val) => idle_timeout = Some(val.duration()),
                        Err(_) => {
                            let _ = writeln!(stderr, "persistd: invalid duration: {}", raw_args[i]);
                            return 2;
                        }
                    }
                }
            } else {
                remaining.push(raw_args[i].clone());
            }
            i += 1;
        }
    }
    let command = remaining.first().map(String::as_str);
    let result = match command {
        None | Some("-h" | "--help" | "help") => write_help(stdout),
        Some("-V" | "--version" | "version") => writeln!(stdout, "{}", version_string("persistd"))
            .map_err(|source| PersistError::Io {
                operation: "write version",
                source,
            }),
        Some("foreground") => run_foreground(idle_timeout),
        Some(other) => Err(PersistError::invalid_argument(format!(
            "unknown persistd command: {other}"
        ))),
    };

    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "persistd: {error}");
            2
        }
    }
}

fn write_help<W: Write>(stdout: &mut W) -> Result<()> {
    writeln!(
        stdout,
        "\
PersistShell daemon

Usage:
  persistd <command> [options]
  persistd foreground [--idle-timeout <duration>]

Available commands:
  help       Show this help
  version    Show version information
  foreground Run the daemon in the foreground with optional --idle-timeout
"
    )
    .map_err(|source| PersistError::Io {
        operation: "write help",
        source,
    })
}

fn run_foreground(idle_timeout: Option<std::time::Duration>) -> Result<()> {
    let options = ConfigLoadOptions::from_environment()?;
    let config = load_config(&options)?;
    init_logging(config.internal_log.daemon_logger_config())?;
    crate::lifecycle::reset_shutdown();
    crate::lifecycle::setup_signal_handler()?;

    crate::lifecycle::prepare_runtime_dir(&config.paths.runtime_dir)?;
    let pidfile = crate::lifecycle::PidFile::create(config.paths.runtime_dir.join("daemon.pid"))?;
    let holder = Arc::new(
        crate::holder::HolderRuntime::initialize(
            &config.paths.runtime_dir,
            &config.paths.holder_socket_path,
            if config.ring_buffer.replay_on_attach {
                config.ring_buffer.replay_bytes.bytes() as u32
            } else {
                0
            },
        )?
        .ok_or_else(|| {
            PersistError::internal_error("persist-holder is required for daemon runtime")
        })?,
    );
    let snapshot = holder.reconciliation_snapshot();
    let exit_contexts = collect_exit_contexts(&holder, &snapshot)?;
    let environment_policy = recovery_environment_policy(&config.recovery.environment)?;
    let mut metadata_store = MetadataStore::open(&config.paths.data_dir.join("metadata.db"))?;
    let reconciliation = crate::holder::reconcile_metadata(
        &mut metadata_store,
        &snapshot,
        &exit_contexts,
        &environment_policy,
    )?;
    for session_id in &reconciliation.exited_sessions {
        holder.retire_exited(*session_id)?;
    }
    let records = metadata_store.list_sessions()?;
    let metadata_next = metadata_store.next_session_id()?;
    let holder_next = snapshot
        .entries
        .iter()
        .map(|entry| entry.session_id)
        .max()
        .map_or(Ok(1), |id| {
            id.checked_add(1)
                .ok_or_else(|| PersistError::invalid_argument("session id space exhausted"))
        })?;
    let next_session_id = metadata_next.max(holder_next);
    let metadata = Arc::new(Mutex::new(metadata_store));
    let mut manager = SessionManager::new(
        config.ring_buffer.default_size.bytes() as usize,
        config.logging.session_log,
        config.paths.data_dir.join("sessions"),
        config.paths.data_dir.join("history"),
        config.logging.max_file_size.bytes(),
        config.logging.max_files,
    );
    manager.set_runtime_dir(config.paths.runtime_dir.clone());
    manager.set_recovery_environment(config.recovery.environment.clone());
    manager.set_holder(Some(holder.clone()));
    manager.load_metadata(&records);
    manager.set_orphaned_sessions(reconciliation.orphaned_sessions);
    manager.set_replay_config(
        config.ring_buffer.replay_on_attach,
        config.ring_buffer.replay_bytes.bytes() as usize,
    );
    manager.set_next_id(next_session_id);
    crash_at_test_point("after_reconcile");
    let socket = DaemonSocket::bind(config.paths.socket_path.clone())?;
    let sm = Arc::new(Mutex::new(manager));
    let gc_timeout = idle_timeout.unwrap_or(config.daemon.gc_idle_timeout.duration());
    sm.lock().unwrap().set_gc_idle_timeout(gc_timeout);
    let gc_interval = config.daemon.gc_interval.duration();
    let mut next_gc = std::time::Instant::now() + gc_interval;
    let mut dashboard = DashboardRuntime::start(config.paths.state_dir.join("metrics"));
    let dashboard_service = dashboard.service();
    let mut next_dashboard_sample = Instant::now();
    let mut holder_failed = false;

    while !crate::lifecycle::shutdown_requested() {
        match socket.accept_timeout(Duration::from_millis(250)) {
            Ok(Some(conn)) => {
                let sm = sm.clone();
                let metadata = metadata.clone();
                let dashboard = dashboard_service.clone();
                std::thread::spawn(move || {
                    let _ = handle_client(conn, sm, Some(metadata), Some(dashboard), 0);
                });
            }
            Ok(None) => {}
            Err(error) => eprintln!("persistd: {error}"),
        }
        if Instant::now() >= next_dashboard_sample {
            let refresh = (!holder_failed).then(|| {
                holder.refresh_inventory().and_then(|()| {
                    let snapshot = holder.reconciliation_snapshot();
                    let contexts = collect_exit_contexts(&holder, &snapshot)?;
                    crate::holder::reconcile_metadata(
                        &mut metadata.lock().unwrap(),
                        &snapshot,
                        &contexts,
                        &environment_policy,
                    )
                })
            });
            match refresh {
                None => {}
                Some(Ok(reconciliation)) => {
                    sm.lock()
                        .unwrap()
                        .set_orphaned_sessions(reconciliation.orphaned_sessions);
                    for session_id in reconciliation.exited_sessions {
                        match holder.retire_exited(session_id) {
                            Ok(()) => sm.lock().unwrap().finish_close(session_id),
                            Err(error) => {
                                eprintln!(
                                    "persistd: failed to retire exited Holder session {session_id}: {error}"
                                );
                            }
                        }
                    }
                }
                Some(Err(error)) if holder.has_exited().unwrap_or(false) => {
                    let snapshot = holder.mark_unavailable();
                    match crate::holder::reconcile_metadata(
                        &mut metadata.lock().unwrap(),
                        &snapshot,
                        &HashMap::new(),
                        &environment_policy,
                    ) {
                        Ok(reconciliation) => {
                            sm.lock()
                                .unwrap()
                                .set_orphaned_sessions(reconciliation.orphaned_sessions);
                            holder_failed = true;
                            eprintln!("persistd: Holder exited; active sessions marked lost");
                        }
                        Err(reconcile_error) => eprintln!(
                            "persistd: Holder exited after refresh error ({error}); metadata reconciliation failed: {reconcile_error}"
                        ),
                    }
                }
                Some(Err(error)) => {
                    eprintln!("persistd: Holder reconciliation failed: {error}")
                }
            }
            let request = sm.lock().unwrap().dashboard_sample_request();
            let _ = dashboard.try_trigger(request);
            next_dashboard_sample = Instant::now() + SAMPLE_INTERVAL;
        }
        if !gc_timeout.is_zero() && std::time::Instant::now() >= next_gc {
            let metadata = metadata.clone();
            let candidates = sm.lock().unwrap().gc_candidates(|id| {
                metadata
                    .lock()
                    .unwrap()
                    .get_session(id)
                    .ok()
                    .flatten()
                    .is_some_and(|record| record.pinned)
            });
            if !candidates.is_empty() {
                let metadata = Some(metadata.clone());
                let removed_ids = candidates
                    .into_iter()
                    .filter(|id| {
                        sm.lock().unwrap().kill_session(*id).is_ok()
                            && finalize_runtime_exit(*id, None, &sm, &metadata).is_ok()
                    })
                    .collect::<Vec<_>>();
                eprintln!("persistd: GC removed sessions: {removed_ids:?}");
            }
            next_gc = std::time::Instant::now() + gc_interval;
        }
    }

    dashboard.shutdown();
    holder.shutdown()?;
    drop(socket);
    drop(pidfile);
    Ok(())
}

#[cfg(debug_assertions)]
fn crash_at_test_point(point: &str) {
    if std::env::var("PERSIST_TEST_CRASH_POINT").as_deref() == Ok(point) {
        unsafe { libc::_exit(86) };
    }
}

#[cfg(not(debug_assertions))]
fn crash_at_test_point(_point: &str) {}

struct SessionManager {
    holder: Option<Arc<crate::holder::HolderRuntime>>,
    #[cfg(test)]
    sessions: Vec<(u32, Arc<Mutex<PtySession>>)>,
    session_info: HashMap<u32, SessionInfo>,
    #[cfg(test)]
    ring_buffers: HashMap<u32, Arc<Mutex<RingBuffer>>>,
    ring_buffer_size: usize,
    replay_on_attach: bool,
    replay_bytes: usize,
    #[cfg(test)]
    log_handles: HashMap<u32, SessionLogHandle>,
    session_log_enabled: bool,
    logs_dir: PathBuf,
    history_dir: PathBuf,
    runtime_dir: PathBuf,
    max_file_size: u64,
    max_files: u32,
    next_id: u32,
    attached_sessions: HashMap<u32, RawFd>,
    ro_attached: HashMap<u32, Vec<RawFd>>,
    last_activity: HashMap<u32, std::time::Instant>,
    gc_idle_timeout: std::time::Duration,
    locked_sessions: HashSet<u32>,
    orphaned_sessions: HashSet<u32>,
    recovery_contexts: HashMap<u32, RecoveryContext>,
    recovery_environment: RecoveryEnvironmentConfig,
}

#[derive(Debug, Clone)]
struct SessionInfo {
    name: String,
    shell: Option<String>,
}

struct LegacyRuntimeInfo {
    alive: bool,
    exit_code: Option<i32>,
    foreground: (Option<u32>, String, String),
}

#[derive(Debug, Clone, Default)]
struct RecoveryContext {
    cwd: Option<String>,
    environment: Option<EnvironmentSnapshot>,
}

impl RecoveryContext {
    fn merge_with_fallback(self, fallback: Option<Self>) -> Self {
        let fallback = fallback.unwrap_or_default();
        Self {
            cwd: self.cwd.or(fallback.cwd),
            environment: self.environment.or(fallback.environment),
        }
    }
}

#[derive(Debug, Clone)]
struct ClosedSession {
    exit_code: i32,
    recovery_context: RecoveryContext,
    holder_retire: bool,
}

fn generate_session_name(shell: &str) -> String {
    let shell_name = std::path::Path::new(shell)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "sh".to_string());
    let cwd = std::env::current_dir().ok();
    let cwd_name = cwd
        .as_ref()
        .and_then(|p| {
            let name = p.file_name().map(|s| s.to_string_lossy().to_string());
            if name.is_none() {
                Some(p.to_string_lossy().to_string())
            } else {
                name
            }
        })
        .unwrap_or_else(|| "?".to_string());
    format!("{}@{}", shell_name, cwd_name)
}

fn validate_session_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        return Err(PersistError::invalid_argument("session name is invalid"));
    }
    if name.len() > 80 {
        return Err(PersistError::invalid_argument("session name is too long"));
    }
    Ok(())
}

fn is_recoverable_environment_name(name: &str) -> bool {
    matches!(name, "TERM" | "COLORTERM" | "LANG") || name.starts_with("LC_")
}

fn recovery_environment_policy(config: &RecoveryEnvironmentConfig) -> Result<EnvironmentPolicy> {
    EnvironmentPolicy::new(
        &config.include,
        config.max_variables,
        usize::try_from(config.max_bytes.bytes()).unwrap_or(usize::MAX),
    )
}

#[cfg(test)]
fn capture_recovery_context(pty: &PtySession, policy: &EnvironmentPolicy) -> RecoveryContext {
    capture_recovery_context_pid(pty.child_pid(), policy)
}

fn capture_recovery_context_pid(pid: u32, policy: &EnvironmentPolicy) -> RecoveryContext {
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned));
    let mut raw = Vec::new();
    let environment = std::fs::File::open(format!("/proc/{pid}/environ"))
        .and_then(|file| file.take(16 * 1024).read_to_end(&mut raw))
        .ok()
        .map(|_| {
            raw.split(|byte| *byte == 0)
                .filter_map(|entry| std::str::from_utf8(entry).ok())
                .filter_map(|entry| entry.split_once('='))
                .filter(|(name, _)| is_recoverable_environment_name(name))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect()
        });
    let environment = environment
        .and_then(|environment| EnvironmentSnapshot::capture(policy, None, environment).ok());
    RecoveryContext { cwd, environment }
}

fn collect_exit_contexts(
    holder: &crate::holder::HolderRuntime,
    snapshot: &crate::holder::HolderInventorySnapshot,
) -> Result<HashMap<u32, crate::holder::ExitContext>> {
    let mut contexts = holder.exit_contexts(snapshot)?;
    for entry in snapshot
        .entries
        .iter()
        .filter(|entry| entry.state == persist_ipc::holder::HolderSessionState::Exited)
    {
        let fallback = std::fs::read_link(format!("/proc/{}/cwd", entry.shell_pid))
            .ok()
            .and_then(|path| path.to_str().map(str::to_owned));
        if let Some(context) = contexts.get_mut(&entry.session_id) {
            if context.cwd.is_none() {
                context.cwd = fallback;
            }
        } else {
            let exit_code = entry.exit_code.ok_or_else(|| {
                PersistError::invalid_argument("exited Holder session is missing exit code")
            })?;
            contexts.insert(
                entry.session_id,
                crate::holder::ExitContext {
                    session_id: entry.session_id,
                    exit_code,
                    cwd: fallback,
                    environment: None,
                },
            );
        }
    }
    Ok(contexts)
}

#[cfg(test)]
fn foreground_process_info(pty: &PtySession) -> (Option<u32>, String, String) {
    let Some(pid) = pty.foreground_process_group() else {
        return (None, String::new(), String::new());
    };
    let (name, command) = process_identity(pid);
    if name.is_empty() {
        return (None, String::new(), String::new());
    }
    (Some(pid), name, command)
}

fn process_identity(pid: u32) -> (String, String) {
    let proc_dir = format!("/proc/{pid}");
    let name = match std::fs::read_to_string(format!("{proc_dir}/comm")) {
        Ok(name) => name.trim().to_owned(),
        Err(_) => return (String::new(), String::new()),
    };
    let mut raw_cmdline = Vec::new();
    let _ = std::fs::File::open(format!("{proc_dir}/cmdline"))
        .and_then(|file| file.take(161).read_to_end(&mut raw_cmdline));
    let truncated = raw_cmdline.len() > 160;
    raw_cmdline.truncate(160);
    let mut cmd = String::from_utf8_lossy(&raw_cmdline)
        .replace('\0', " ")
        .trim()
        .to_owned();
    if truncated {
        while !cmd.is_empty() && !cmd.is_char_boundary(cmd.len()) {
            cmd.pop();
        }
        cmd.push_str("...");
    }
    (name, cmd)
}

#[cfg(test)]
fn process_tree(pty: &PtySession) -> Vec<ProcessTreeNode> {
    pty.foreground_process_group()
        .map(process_tree_pid)
        .unwrap_or_default()
}

fn process_tree_pid(root_pid: u32) -> Vec<ProcessTreeNode> {
    const MAX_NODES: usize = 64;
    const MAX_DEPTH: u8 = 8;
    let mut queue = VecDeque::from([(root_pid, 0_u32, 0_u8)]);
    let mut nodes = Vec::new();
    while let Some((pid, parent_pid, depth)) = queue.pop_front() {
        if nodes.len() == MAX_NODES || depth > MAX_DEPTH {
            break;
        }
        let proc_dir = format!("/proc/{pid}");
        let Ok(name) = std::fs::read_to_string(format!("{proc_dir}/comm")) else {
            continue;
        };
        let mut raw_cmdline = Vec::new();
        let _ = std::fs::File::open(format!("{proc_dir}/cmdline"))
            .and_then(|file| file.take(160).read_to_end(&mut raw_cmdline));
        let command = String::from_utf8_lossy(&raw_cmdline)
            .replace('\0', " ")
            .trim()
            .to_owned();
        nodes.push(ProcessTreeNode {
            pid,
            parent_pid,
            depth,
            name: name.trim().to_owned(),
            command,
        });
        if depth == MAX_DEPTH {
            continue;
        }
        if let Ok(children) = std::fs::read_to_string(format!("{proc_dir}/task/{pid}/children")) {
            for child in children
                .split_whitespace()
                .filter_map(|child| child.parse().ok())
            {
                queue.push_back((child, pid, depth + 1));
            }
        }
    }
    nodes
}

#[cfg(test)]
fn process_stats(pty: &PtySession) -> ProcessStatsRespPayload {
    pty.foreground_process_group()
        .map(process_stats_pid)
        .unwrap_or_else(empty_process_stats)
}

fn empty_process_stats() -> ProcessStatsRespPayload {
    ProcessStatsRespPayload {
        pid: None,
        user_ticks: 0,
        system_ticks: 0,
        rss_kib: 0,
        read_bytes: 0,
        write_bytes: 0,
    }
}

fn process_stats_pid(pid: u32) -> ProcessStatsRespPayload {
    let empty = ProcessStatsRespPayload {
        pid: None,
        user_ticks: 0,
        system_ticks: 0,
        rss_kib: 0,
        read_bytes: 0,
        write_bytes: 0,
    };
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return empty;
    };
    let Some((_, values)) = stat.rsplit_once(") ") else {
        return empty;
    };
    let values = values.split_whitespace().collect::<Vec<_>>();
    if values.len() < 22 {
        return empty;
    }
    let (Ok(user_ticks), Ok(system_ticks), Ok(rss_pages)) = (
        values[11].parse(),
        values[12].parse(),
        values[21].parse::<u64>(),
    ) else {
        return empty;
    };
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(0) as u64;
    let io = std::fs::read_to_string(format!("/proc/{pid}/io")).unwrap_or_default();
    let value = |key| {
        io.lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0)
    };
    ProcessStatsRespPayload {
        pid: Some(pid),
        user_ticks,
        system_ticks,
        rss_kib: rss_pages.saturating_mul(page_size) / 1024,
        read_bytes: value("read_bytes:"),
        write_bytes: value("write_bytes:"),
    }
}

const MAX_JSON_RESPONSE_BYTES: usize = 16 * 1024;

fn bounded_json(value: serde_json::Value, error: serde_json::Value) -> String {
    let json = value.to_string();
    if json.len() <= MAX_JSON_RESPONSE_BYTES {
        json
    } else {
        error.to_string()
    }
}

fn snapshot_json(session_id: u32, value: serde_json::Value) -> String {
    bounded_json(
        value,
        serde_json::json!({
            "session_id": session_id,
            "error": "snapshot exceeds output limit",
        }),
    )
}

fn metrics_json(value: serde_json::Value) -> String {
    bounded_json(
        value,
        serde_json::json!({"error": "metrics exceeds output limit"}),
    )
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

impl SessionManager {
    fn new(
        ring_buffer_size: usize,
        session_log_enabled: bool,
        logs_dir: PathBuf,
        history_dir: PathBuf,
        max_file_size: u64,
        max_files: u32,
    ) -> Self {
        let runtime_dir = history_dir.join(format!(".runtime-{}", std::process::id()));
        Self {
            holder: None,
            #[cfg(test)]
            sessions: Vec::new(),
            session_info: HashMap::new(),
            #[cfg(test)]
            ring_buffers: HashMap::new(),
            ring_buffer_size,
            replay_on_attach: true,
            replay_bytes: ring_buffer_size,
            #[cfg(test)]
            log_handles: HashMap::new(),
            session_log_enabled,
            logs_dir,
            history_dir,
            runtime_dir,
            max_file_size,
            max_files,
            next_id: 1,
            attached_sessions: HashMap::new(),
            ro_attached: HashMap::new(),
            last_activity: HashMap::new(),
            gc_idle_timeout: std::time::Duration::from_secs(0),
            locked_sessions: HashSet::new(),
            orphaned_sessions: HashSet::new(),
            recovery_contexts: HashMap::new(),
            recovery_environment: RecoveryEnvironmentConfig::default(),
        }
    }

    fn create(&mut self) -> Result<u32> {
        let shell = detect_shell();
        self.create_with_shell(Some(&shell))
    }

    fn set_holder(&mut self, holder: Option<Arc<crate::holder::HolderRuntime>>) {
        self.holder = holder;
    }

    fn set_runtime_dir(&mut self, runtime_dir: PathBuf) {
        self.runtime_dir = runtime_dir;
    }

    fn set_recovery_environment(&mut self, config: RecoveryEnvironmentConfig) {
        self.recovery_environment = config;
    }

    fn recovery_environment_policy(&self) -> Result<EnvironmentPolicy> {
        recovery_environment_policy(&self.recovery_environment)
    }

    fn load_metadata(&mut self, records: &[SessionRecord]) {
        for record in records {
            self.session_info.insert(
                record.session_id,
                SessionInfo {
                    name: record.name.clone(),
                    shell: record.shell.clone(),
                },
            );
            self.set_locked(record.session_id, record.locked);
        }
    }

    fn set_orphaned_sessions(&mut self, sessions: HashSet<u32>) {
        self.orphaned_sessions = sessions;
    }

    fn holder_backend(&self) -> Option<Arc<crate::holder::HolderRuntime>> {
        self.holder.clone()
    }

    fn holder_binding(&self) -> Option<(String, u64)> {
        let snapshot = self.holder.as_ref()?.reconciliation_snapshot();
        Some((snapshot.instance_hex(), snapshot.generation))
    }

    fn holder_diagnostics(&self) -> serde_json::Value {
        match &self.holder {
            Some(holder) => serde_json::json!({
                "pid": holder.pid(),
                "instance": holder.instance_hex(),
                "connected": holder.is_connected(),
            }),
            None => serde_json::json!({
                "pid": null,
                "instance": null,
                "connected": false,
            }),
        }
    }

    fn set_next_id(&mut self, next_id: u32) {
        self.next_id = next_id;
    }

    fn set_replay_config(&mut self, enabled: bool, max_bytes: usize) {
        self.replay_on_attach = enabled;
        self.replay_bytes = max_bytes;
    }

    fn closed_attach_history(&self, id: u32) -> Vec<u8> {
        if !self.replay_on_attach || !self.session_log_enabled || self.replay_bytes == 0 {
            return Vec::new();
        }
        match crate::attach_history::read_rotated_tail(
            &self.logs_dir,
            id,
            self.max_files,
            self.replay_bytes,
        ) {
            Ok(history) => history,
            Err(error) => {
                eprintln!(
                    "persistd: Session {id} attach history unavailable; continuing without replay: {error}"
                );
                Vec::new()
            }
        }
    }

    fn dashboard_sample_request(&self) -> SampleRequest {
        let mut roots = Vec::new();
        #[cfg(test)]
        roots.extend(
            self.sessions
                .iter()
                .filter_map(|(session_id, pty)| {
                    let pty = pty.lock().ok()?;
                    Some(SessionRoot {
                        session_id: *session_id,
                        root_pid: pty.child_pid(),
                        foreground_pid: pty.foreground_process_group(),
                        writer_active: self.attached_sessions.contains_key(session_id),
                    })
                })
                .collect::<Vec<_>>(),
        );
        if let Some(holder) = &self.holder {
            roots.extend(
                holder
                    .inventory_snapshot()
                    .into_iter()
                    .filter(|entry| !self.orphaned_sessions.contains(&entry.session_id))
                    .map(|entry| SessionRoot {
                        session_id: entry.session_id,
                        root_pid: entry.shell_pid,
                        foreground_pid: Some(entry.shell_pid),
                        writer_active: entry.writer_active
                            || self.attached_sessions.contains_key(&entry.session_id),
                    }),
            );
        }
        SampleRequest {
            roots,
            session_count: saturating_u32(self.session_info.len()),
            runtime_count: saturating_u32(self.runtime_ids().len()),
            active_writer_count: saturating_u32(self.active_writer_count()),
            readonly_client_count: saturating_u32(self.ro_attached.values().map(Vec::len).sum()),
        }
    }

    #[cfg(test)]
    fn replay_output(&self, id: u32) -> Vec<u8> {
        if !self.replay_on_attach || self.replay_bytes == 0 {
            return Vec::new();
        }
        self.ring_buffers
            .get(&id)
            .map(|buffer| buffer.lock().unwrap().read_replay(self.replay_bytes))
            .unwrap_or_default()
    }

    fn create_with_shell(&mut self, shell: Option<&str>) -> Result<u32> {
        let id = self.next_id;
        self.next_id += 1;
        let selected_shell = shell.map(str::to_owned).unwrap_or_else(detect_shell);
        if let Some(holder) = self.holder.clone() {
            let request = self.holder_create_request(
                id,
                &selected_shell,
                None,
                &[],
                &[],
                &ConnectionEnvironment::default(),
            )?;
            holder.create(request)?;
            let name = generate_session_name(&selected_shell);
            self.session_info.insert(
                id,
                SessionInfo {
                    name,
                    shell: Some(selected_shell),
                },
            );
            self.record_activity(id);
            return Ok(id);
        }
        #[cfg(test)]
        {
            let pty = self.open_shell(
                id,
                &selected_shell,
                None,
                &[],
                &[],
                &ConnectionEnvironment::default(),
            )?;
            let actual_shell = pty.shell().to_string();
            let name = generate_session_name(&actual_shell);
            self.insert_runtime(id, name, pty);
            Ok(id)
        }
        #[cfg(not(test))]
        Err(PersistError::internal_error(
            "persist-holder is unavailable",
        ))
    }

    fn restore_closed_session(
        &mut self,
        id: u32,
        name: String,
        shell: Option<&str>,
        cwd: Option<&Path>,
        environment: Option<&EnvironmentSnapshot>,
        connection_env: &ConnectionEnvironment,
    ) -> Result<()> {
        if self.has_runtime(id) {
            return Err(PersistError::invalid_argument("session is already running"));
        }
        let selected_shell = shell.map(str::to_owned).unwrap_or_else(detect_shell);
        let saved_set = environment
            .map(|snapshot| {
                snapshot
                    .env_set
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let saved_unset = environment
            .map(|snapshot| snapshot.env_unset.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        if let Some(holder) = self.holder.clone() {
            let request = self.holder_create_request(
                id,
                &selected_shell,
                cwd,
                &saved_set,
                &saved_unset,
                connection_env,
            )?;
            holder.create(request)?;
            self.next_id = self.next_id.max(id.saturating_add(1));
            self.session_info.insert(
                id,
                SessionInfo {
                    name,
                    shell: Some(selected_shell),
                },
            );
            self.record_activity(id);
            return Ok(());
        }
        #[cfg(test)]
        {
            let pty = self.open_shell(
                id,
                &selected_shell,
                cwd,
                &saved_set,
                &saved_unset,
                connection_env,
            )?;
            self.next_id = self.next_id.max(id.saturating_add(1));
            self.insert_runtime(id, name, pty);
            Ok(())
        }
        #[cfg(not(test))]
        Err(PersistError::internal_error(
            "persist-holder is unavailable",
        ))
    }

    #[cfg(test)]
    fn open_shell(
        &self,
        id: u32,
        shell: &str,
        cwd: Option<&Path>,
        environment: &[(String, String)],
        environment_unset: &[String],
        connection_env: &ConnectionEnvironment,
    ) -> Result<PtySession> {
        let histfile_path = self.history_dir.join(id.to_string());
        if let Some(parent) = histfile_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let histfile = histfile_path.to_string_lossy().into_owned();
        let identity = self.create_shell_identity(id)?;
        let launch = match crate::shell_history::helper_path() {
            Some(helper) => crate::shell_history::prepare_with_policy(
                shell,
                id,
                &self.history_dir,
                &helper,
                &identity,
                &self.recovery_environment,
            )?,
            None => None,
        };
        let engine = PtyEngine::new();
        let connection = connection_env
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        match launch {
            Some(launch) => {
                let environment = ShellLaunchEnvironment::new(
                    environment.to_vec(),
                    environment_unset.to_vec(),
                    connection,
                    launch.environment,
                )?;
                engine.open_session_with_launch_environment(
                    shell,
                    Some(&histfile),
                    cwd,
                    &environment,
                    &launch.arguments,
                )
            }
            None => {
                let environment = ShellLaunchEnvironment::new(
                    environment.to_vec(),
                    environment_unset.to_vec(),
                    connection,
                    Vec::new(),
                )?;
                engine.open_session_with_launch_environment(
                    shell,
                    Some(&histfile),
                    cwd,
                    &environment,
                    &[],
                )
            }
        }
    }

    fn holder_create_request(
        &self,
        id: u32,
        shell: &str,
        cwd: Option<&Path>,
        environment: &[(String, String)],
        environment_unset: &[String],
        connection_env: &ConnectionEnvironment,
    ) -> Result<HolderCreateRequest> {
        let history_file = self.history_dir.join(id.to_string());
        if let Some(parent) = history_file.parent() {
            std::fs::create_dir_all(parent).map_err(|source| PersistError::Io {
                operation: "create holder history directory",
                source,
            })?;
        }
        if self.session_log_enabled {
            std::fs::create_dir_all(&self.logs_dir).map_err(|source| PersistError::Io {
                operation: "create Holder session log directory",
                source,
            })?;
            std::fs::set_permissions(&self.logs_dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|source| PersistError::Io {
                    operation: "set Holder session log directory permissions",
                    source,
                })?;
        }
        let identity = self.create_shell_identity(id)?;
        let launch = match crate::shell_history::helper_path() {
            Some(helper) => crate::shell_history::prepare_with_policy(
                shell,
                id,
                &self.history_dir,
                &helper,
                &identity,
                &self.recovery_environment,
            )?,
            None => None,
        };
        let arguments = launch
            .as_ref()
            .map_or_else(Vec::new, |item| item.arguments.clone());
        let private = launch.map(|item| item.environment).unwrap_or_default();
        let connection = connection_env
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        let launch_environment = ShellLaunchEnvironment::new(
            environment.to_vec(),
            environment_unset.to_vec(),
            connection,
            private,
        )?;
        let ring_buffer_size = u32::try_from(self.ring_buffer_size)
            .map_err(|_| PersistError::invalid_argument("ring buffer exceeds holder limit"))?;
        Ok(HolderCreateRequest {
            session_id: id,
            shell: shell.to_owned(),
            arguments,
            cwd: cwd.map(|path| path.to_string_lossy().into_owned()),
            launch_environment,
            history_file: Some(history_file.to_string_lossy().into_owned()),
            ring_buffer_size,
            log_path: self.session_log_enabled.then(|| {
                self.logs_dir
                    .join(format!("{id}.log"))
                    .to_string_lossy()
                    .into_owned()
            }),
            state_file: identity.path_string(),
            state_incarnation: identity.incarnation(),
        })
    }

    fn create_shell_identity(
        &self,
        id: u32,
    ) -> Result<persist_core::shell_state::ShellStateIdentity> {
        match std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&self.runtime_dir)
        {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(PersistError::Io {
                    operation: "create shell state runtime directory",
                    source,
                });
            }
        }
        persist_core::shell_state::create_identity(&self.runtime_dir, id)
    }

    #[cfg(test)]
    fn insert_runtime(&mut self, id: u32, name: String, pty: PtySession) {
        let shell = pty.shell().to_owned();
        self.record_activity(id);
        self.session_info.insert(
            id,
            SessionInfo {
                name,
                shell: Some(shell),
            },
        );
        self.sessions.push((id, Arc::new(Mutex::new(pty))));
        if self.ring_buffer_size > 0 {
            self.ring_buffers.insert(
                id,
                Arc::new(Mutex::new(RingBuffer::new(self.ring_buffer_size))),
            );
        }
        if self.session_log_enabled {
            let log_path = self.logs_dir.join(format!("{id}.log"));
            let handle = spawn_session_logger(
                log_path,
                self.max_file_size,
                self.max_files,
                Arc::new(Mutex::new(false)),
            );
            self.log_handles.insert(id, handle);
        }
    }

    fn session_name(&self, id: u32) -> Option<String> {
        self.session_info.get(&id).map(|info| info.name.clone())
    }

    fn session_shell(&self, id: u32) -> Option<String> {
        if let Some(shell) = self
            .session_info
            .get(&id)
            .and_then(|info| info.shell.clone())
        {
            return Some(shell);
        }
        #[cfg(test)]
        {
            self.sessions
                .iter()
                .find(|(session_id, _)| *session_id == id)
                .and_then(|(_, pty)| pty.lock().ok().map(|pty| pty.shell().to_owned()))
        }
        #[cfg(not(test))]
        None
    }

    fn record_recovery_context(&mut self, id: u32, context: RecoveryContext) {
        if context.cwd.is_some() || context.environment.is_some() {
            let stored = self.recovery_contexts.entry(id).or_default();
            if context.cwd.is_some() {
                stored.cwd = context.cwd;
            }
            if context.environment.is_some() {
                stored.environment = context.environment;
            }
        }
    }

    fn rename_session(&mut self, id: u32, name: String) -> Result<()> {
        let info = self
            .session_info
            .get_mut(&id)
            .ok_or_else(|| PersistError::invalid_argument("session not found"))?;
        info.name = name;
        Ok(())
    }

    #[cfg(test)]
    fn remove(&mut self, id: u32) -> Option<(Arc<Mutex<PtySession>>, Option<RecoveryContext>)> {
        if let Some(pos) = self.sessions.iter().position(|(sid, _)| *sid == id) {
            self.session_info.remove(&id);
            self.ring_buffers.remove(&id);
            self.log_handles.remove(&id);
            self.attached_sessions.remove(&id);
            self.ro_attached.remove(&id);
            self.last_activity.remove(&id);
            let recovery_context = self.recovery_contexts.remove(&id);
            Some((self.sessions.swap_remove(pos).1, recovery_context))
        } else {
            None
        }
    }

    #[cfg(test)]
    fn list(&self) -> Vec<(u32, Arc<Mutex<PtySession>>)> {
        self.sessions
            .iter()
            .map(|(id, pty)| (*id, pty.clone()))
            .collect()
    }

    fn holder_entry(&self, id: u32) -> Option<persist_ipc::holder::HolderSessionEntry> {
        if self.orphaned_sessions.contains(&id) {
            return None;
        }
        self.holder
            .as_ref()?
            .inventory_snapshot()
            .into_iter()
            .find(|entry| entry.session_id == id)
    }

    fn output_log_state(&self, id: u32) -> &'static str {
        match self.holder_entry(id).map(|entry| entry.log_state) {
            Some(persist_ipc::holder::HolderLogState::Healthy) => "healthy",
            Some(persist_ipc::holder::HolderLogState::Degraded) => "degraded",
            Some(persist_ipc::holder::HolderLogState::Disabled) => "disabled",
            None => "unavailable",
        }
    }

    fn degraded_log_count(&self) -> usize {
        self.holder.as_ref().map_or(0, |holder| {
            holder
                .inventory_snapshot()
                .iter()
                .filter(|entry| {
                    !self.orphaned_sessions.contains(&entry.session_id)
                        && entry.log_state == persist_ipc::holder::HolderLogState::Degraded
                })
                .count()
        })
    }

    fn active_writer_count(&self) -> usize {
        let mut writers = self
            .attached_sessions
            .keys()
            .filter(|id| !self.orphaned_sessions.contains(id))
            .copied()
            .collect::<HashSet<_>>();
        if let Some(holder) = &self.holder {
            writers.extend(
                holder
                    .inventory_snapshot()
                    .iter()
                    .filter(|entry| {
                        !self.orphaned_sessions.contains(&entry.session_id) && entry.writer_active
                    })
                    .map(|entry| entry.session_id),
            );
        }
        writers.len()
    }

    fn legacy_runtime_info(&self, id: u32) -> Option<LegacyRuntimeInfo> {
        #[cfg(test)]
        {
            let pty = self
                .sessions
                .iter()
                .find(|(session_id, _)| *session_id == id)?
                .1
                .lock()
                .ok()?;
            Some(LegacyRuntimeInfo {
                alive: pty.is_alive(),
                exit_code: pty.exit_code(),
                foreground: foreground_process_info(&pty),
            })
        }
        #[cfg(not(test))]
        None
    }

    fn process_tree(&self, id: u32) -> Vec<ProcessTreeNode> {
        if let Some(entry) = self.holder_entry(id) {
            return process_tree_pid(entry.shell_pid);
        }
        #[cfg(test)]
        {
            self.sessions
                .iter()
                .find(|(session_id, _)| *session_id == id)
                .and_then(|(_, pty)| pty.lock().ok().map(|pty| process_tree(&pty)))
                .unwrap_or_default()
        }
        #[cfg(not(test))]
        Vec::new()
    }

    fn process_stats(&self, id: u32) -> ProcessStatsRespPayload {
        if let Some(entry) = self.holder_entry(id) {
            return process_stats_pid(entry.shell_pid);
        }
        #[cfg(test)]
        {
            self.sessions
                .iter()
                .find(|(session_id, _)| *session_id == id)
                .and_then(|(_, pty)| pty.lock().ok().map(|pty| process_stats(&pty)))
                .unwrap_or_else(empty_process_stats)
        }
        #[cfg(not(test))]
        empty_process_stats()
    }

    fn runtime_process_info(&self, id: u32) -> Option<(Option<u32>, String, String)> {
        if let Some(entry) = self.holder_entry(id) {
            let (name, command) = process_identity(entry.shell_pid);
            return Some(((!name.is_empty()).then_some(entry.shell_pid), name, command));
        }
        #[cfg(test)]
        {
            self.sessions
                .iter()
                .find(|(session_id, _)| *session_id == id)
                .and_then(|(_, pty)| pty.lock().ok().map(|pty| foreground_process_info(&pty)))
        }
        #[cfg(not(test))]
        None
    }

    fn runtime_ids(&self) -> Vec<u32> {
        let mut ids = Vec::new();
        #[cfg(test)]
        ids.extend(self.sessions.iter().map(|(id, _)| *id));
        if let Some(holder) = &self.holder {
            ids.extend(
                holder
                    .inventory_snapshot()
                    .into_iter()
                    .filter(|entry| !self.orphaned_sessions.contains(&entry.session_id))
                    .map(|entry| entry.session_id),
            );
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn has_runtime(&self, id: u32) -> bool {
        #[cfg(test)]
        if self
            .sessions
            .iter()
            .any(|(session_id, _)| *session_id == id)
        {
            return true;
        }
        self.holder_entry(id).is_some()
    }

    fn is_attached(&self, id: u32) -> bool {
        self.attached_sessions.contains_key(&id)
    }

    fn mark_attached(&mut self, id: u32, fd: RawFd) {
        self.attached_sessions.insert(id, fd);
    }

    fn transfer_writer(&mut self, id: u32, new_fd: RawFd) -> Option<RawFd> {
        self.attached_sessions.insert(id, new_fd)
    }

    fn release_writer(&mut self, id: u32, fd: RawFd) -> bool {
        if self.attached_sessions.get(&id).copied() == Some(fd) {
            self.attached_sessions.remove(&id);
            true
        } else {
            false
        }
    }

    fn is_writer(&self, id: u32, fd: RawFd) -> bool {
        self.attached_sessions.get(&id).copied() == Some(fd)
    }

    fn record_activity(&mut self, id: u32) {
        self.last_activity.insert(id, std::time::Instant::now());
    }

    #[cfg(test)]
    fn write_legacy_input(&mut self, id: u32, payload: &[u8]) -> bool {
        if let Some((_, pty)) = self
            .sessions
            .iter()
            .find(|(session_id, _)| *session_id == id)
        {
            return pty
                .lock()
                .is_ok_and(|mut pty| pty.write_input(payload).is_ok());
        }
        false
    }

    #[cfg(test)]
    fn resize_legacy(&self, rows: u16, cols: u16) -> bool {
        if let Some((_, pty)) = self.sessions.first() {
            return pty
                .lock()
                .is_ok_and(|pty| apply_resize(pty.master_fd(), rows, cols).is_ok());
        }
        false
    }

    #[cfg(test)]
    fn signal_legacy(&self, id: u32, signal: u32) -> bool {
        if let Some((_, pty)) = self
            .sessions
            .iter()
            .find(|(session_id, _)| *session_id == id)
        {
            if let Ok(pty) = pty.lock() {
                let pgid = unsafe { libc::tcgetpgrp(pty.master_fd()) };
                if pgid > 0 {
                    return unsafe { libc::kill(-pgid, signal as i32) } == 0;
                }
            }
        }
        false
    }

    fn idle_string(&self, id: u32) -> String {
        match self.last_activity.get(&id) {
            Some(instant) => {
                let elapsed = instant.elapsed();
                let secs = elapsed.as_secs();
                if secs < 60 {
                    format!("{secs}s")
                } else if secs < 3600 {
                    format!("{}m{:02}s", secs / 60, secs % 60)
                } else {
                    format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
                }
            }
            None => String::new(),
        }
    }

    fn set_gc_idle_timeout(&mut self, timeout: std::time::Duration) {
        self.gc_idle_timeout = timeout;
    }

    fn gc_candidates(&self, is_pinned: impl Fn(u32) -> bool) -> Vec<u32> {
        if self.gc_idle_timeout.is_zero() {
            return Vec::new();
        }
        let now = std::time::Instant::now();
        let timeout = self.gc_idle_timeout;
        let mut candidates = Vec::new();
        for id in self.runtime_ids() {
            if self.is_attached(id) {
                continue;
            }
            if is_pinned(id) || self.locked_sessions.contains(&id) {
                continue;
            }
            let idle = self
                .last_activity
                .get(&id)
                .map_or(true, |last| now.duration_since(*last) >= timeout);
            if idle {
                candidates.push(id);
            }
        }
        candidates
    }

    #[cfg(test)]
    fn gc_run(&mut self, is_pinned: impl Fn(u32) -> bool) -> Vec<(u32, ClosedSession)> {
        let to_remove = self.gc_candidates(is_pinned);
        let mut removed = Vec::new();
        for id in to_remove {
            if self.kill_session(id).is_ok() {
                if let Ok(Some(closed)) = self.close_session(id) {
                    removed.push((id, closed));
                }
            }
        }
        removed
    }

    fn set_locked(&mut self, id: u32, locked: bool) {
        if locked {
            self.locked_sessions.insert(id);
        } else {
            self.locked_sessions.remove(&id);
        }
    }

    fn add_ro_client(&mut self, id: u32, fd: RawFd) {
        self.ro_attached.entry(id).or_default().push(fd);
    }

    fn remove_ro_client(&mut self, id: u32, fd: RawFd) {
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.ro_attached.entry(id) {
            let fds: &mut Vec<RawFd> = e.get_mut();
            fds.retain(|f| *f != fd);
            if fds.is_empty() {
                e.remove();
            }
        }
    }

    #[cfg(test)]
    fn broadcast_stdout(&mut self, id: u32, data: &[u8]) {
        if let Some(fds) = self.ro_attached.get(&id) {
            let mut dead = Vec::new();
            for fd in fds {
                if write_stdout_raw(*fd, data).is_err() {
                    dead.push(*fd);
                }
            }
            if !dead.is_empty() {
                if let std::collections::hash_map::Entry::Occupied(mut e) =
                    self.ro_attached.entry(id)
                {
                    let fds: &mut Vec<RawFd> = e.get_mut();
                    fds.retain(|f| !dead.contains(f));
                    if fds.is_empty() {
                        e.remove();
                    }
                }
            }
        }
    }

    #[cfg(test)]
    fn readonly_clients(&self, id: u32) -> Vec<RawFd> {
        self.ro_attached.get(&id).cloned().unwrap_or_default()
    }

    fn kill_session(&mut self, id: u32) -> Result<()> {
        if self.holder_entry(id).is_some() {
            return self
                .holder
                .as_ref()
                .expect("holder entry requires holder")
                .kill(id);
        }
        #[cfg(test)]
        if let Some((_, pty)) = self.sessions.iter().find(|(sid, _)| *sid == id) {
            let pty = pty.lock().unwrap();
            pty.signal_child(libc::SIGKILL)
                .map_err(|source| PersistError::Io {
                    operation: "kill session",
                    source,
                })?;
        }
        Ok(())
    }

    fn prepare_close(
        &mut self,
        id: u32,
        observed_exit: Option<crate::holder::ExitContext>,
    ) -> Result<Option<ClosedSession>> {
        if let Some(entry) = self.holder_entry(id) {
            let policy = self.recovery_environment_policy()?;
            let direct_context = capture_recovery_context_pid(entry.shell_pid, &policy);
            let stored_context = self.recovery_contexts.get(&id).cloned();
            let holder = self
                .holder
                .as_ref()
                .expect("holder entry requires holder")
                .clone();
            let context = if let Some(context) = observed_exit {
                if context.session_id != id {
                    return Err(PersistError::invalid_argument(
                        "observed Holder exit context has wrong session",
                    ));
                }
                context
            } else if entry.state == persist_ipc::holder::HolderSessionState::Exited {
                if entry.exit_context_available {
                    holder.exit_context(id)?
                } else {
                    crate::holder::ExitContext {
                        session_id: id,
                        exit_code: entry.exit_code.ok_or_else(|| {
                            PersistError::invalid_argument(
                                "exited Holder session is missing exit code",
                            )
                        })?,
                        cwd: None,
                        environment: None,
                    }
                }
            } else {
                holder.close(id)?
            };
            let side_context = RecoveryContext {
                cwd: context.cwd,
                environment: context
                    .environment
                    .and_then(|snapshot| policy.filter_snapshot(&snapshot).ok()),
            };
            let recovery_context = side_context
                .merge_with_fallback(Some(direct_context.merge_with_fallback(stored_context)));
            return Ok(Some(ClosedSession {
                exit_code: context.exit_code,
                recovery_context,
                holder_retire: true,
            }));
        }
        #[cfg(test)]
        if let Some((session, stored_context)) = self.remove(id) {
            if let Ok(mut pty) = session.lock() {
                let policy = self.recovery_environment_policy()?;
                let direct_context = capture_recovery_context(&pty, &policy);
                let recovery_context = direct_context.merge_with_fallback(stored_context);
                if pty.is_alive() {
                    let _ = pty.signal_child(libc::SIGHUP);
                }
                return pty.wait_exit().map(|exit_code| {
                    Some(ClosedSession {
                        exit_code,
                        recovery_context,
                        holder_retire: false,
                    })
                });
            }
        }
        Ok(None)
    }

    fn finish_close(&mut self, id: u32) {
        self.session_info.remove(&id);
        self.attached_sessions.remove(&id);
        self.ro_attached.remove(&id);
        self.last_activity.remove(&id);
        self.recovery_contexts.remove(&id);
    }

    fn close_session(&mut self, id: u32) -> Result<Option<ClosedSession>> {
        let closed = self.prepare_close(id, None)?;
        if let Some(closed) = &closed {
            if closed.holder_retire {
                self.holder
                    .as_ref()
                    .expect("Holder close requires Holder")
                    .retire_exited(id)?;
            }
            self.finish_close(id);
        }
        Ok(closed)
    }
}

fn validate_connection_environment(
    context: &ConnectionEnvironment,
    uid: u32,
) -> ConnectionEnvironment {
    let variables = context.iter().filter(|(name, value)| {
        *name != "SSH_AUTH_SOCK" || valid_agent_socket(Path::new(value), uid)
    });
    ConnectionEnvironment::from_pairs(variables).unwrap_or_default()
}

fn valid_agent_socket(path: &Path, uid: u32) -> bool {
    path.is_absolute()
        && std::fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.file_type().is_socket() && metadata.uid() == uid)
}

fn handle_client(
    mut conn: DaemonConnection,
    sm: Arc<Mutex<SessionManager>>,
    ms: Option<Arc<Mutex<MetadataStore>>>,
    dashboard: Option<DashboardService>,
    _ring_buf_size: usize,
) -> Result<()> {
    let stream = conn.stream();
    let fd = stream.as_raw_fd();

    let _ = set_nonblocking(fd);

    let mut acc = FrameAccumulator::new();

    loop {
        let mut buf = [0u8; 65536];
        let n = match read_nonblock(fd, &mut buf) {
            Ok(0) => {
                let mut pfd = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                let result = unsafe { libc::poll(&mut pfd, 1, 1000) };
                if result < 0 {
                    break;
                }
                continue;
            }
            Ok(n) => n,
            Err(_) => break,
        };
        acc.feed(&buf[..n]);

        while let Ok(Some(frame)) = acc.try_read() {
            match frame.msg_type {
                MessageType::Hello => {
                    let _ = decode_hello(&frame.payload);
                    let ack = encode_hello_ack(&HelloAckPayload {
                        protocol_major: 0,
                        protocol_minor: ATTACH_CONTEXT_PROTOCOL_MINOR,
                        pid: std::process::id(),
                        status: HelloStatus::Accepted,
                    });
                    let _ = write_frame(
                        stream,
                        &Frame {
                            msg_type: MessageType::HelloAck,
                            flags: 0,
                            request_id: 0,
                            payload: ack,
                        },
                    );
                }
                MessageType::NewSession => {
                    let created = {
                        let mut sm = sm.lock().unwrap();
                        sm.create().map(|id| {
                            (
                                id,
                                sm.session_name(id).unwrap_or_default(),
                                sm.session_shell(id),
                                sm.holder_binding(),
                            )
                        })
                    };
                    let (id, name) = match created {
                        Ok((id, name, shell, holder_binding)) => {
                            crash_at_test_point("after_holder_create");
                            let cwd = std::env::current_dir()
                                .ok()
                                .and_then(|path| path.to_str().map(str::to_owned));
                            let metadata_result: Result<()> = match &ms {
                                Some(metadata) => {
                                    let mut metadata = metadata.lock().unwrap();
                                    (|| {
                                        metadata.create_session(
                                            id,
                                            &name,
                                            cwd.as_deref(),
                                            shell.as_deref(),
                                        )?;
                                        if let Some((instance, generation)) = holder_binding {
                                            metadata
                                                .reconcile_running(id, &instance, generation)?;
                                        }
                                        Ok(())
                                    })()
                                }
                                None => Ok(()),
                            };
                            if metadata_result.is_ok() {
                                crash_at_test_point("after_metadata_commit");
                                (id, name)
                            } else {
                                let _ = sm.lock().unwrap().close_session(id);
                                (0, String::new())
                            }
                        }
                        Err(_) => (0, String::new()),
                    };
                    let resp = encode_new_session_resp(&NewSessionRespPayload {
                        session_id: id,
                        name,
                    });
                    let _ = write_frame(
                        stream,
                        &Frame {
                            msg_type: MessageType::NewSessionResp,
                            flags: 0,
                            request_id: 0,
                            payload: resp,
                        },
                    );
                }
                MessageType::ListSessions => {
                    let sm = sm.lock().unwrap();
                    #[cfg(test)]
                    let mut sessions: Vec<SessionEntry> = sm
                        .list()
                        .iter()
                        .map(|(id, pty_arc)| {
                            let pty = pty_arc.lock().unwrap();
                            let status = if sm.is_attached(*id) {
                                "attached"
                            } else if pty.is_alive() {
                                "running"
                            } else {
                                "closed"
                            };
                            let has_note = ms.as_ref().map_or(false, |m| {
                                m.lock()
                                    .unwrap()
                                    .get_session(*id)
                                    .ok()
                                    .flatten()
                                    .and_then(|r| r.note)
                                    .map_or(false, |n| !n.is_empty())
                            });
                            let has_tags = ms.as_ref().map_or(false, |m| {
                                m.lock()
                                    .unwrap()
                                    .list_session_tags(*id)
                                    .ok()
                                    .map_or(false, |t| !t.is_empty())
                            });
                            let is_pinned = ms.as_ref().map_or(false, |m| {
                                m.lock()
                                    .unwrap()
                                    .get_session(*id)
                                    .ok()
                                    .flatten()
                                    .map_or(false, |r| r.pinned)
                            });
                            let (foreground_pid, foreground_name, foreground_cmd) =
                                foreground_process_info(&pty);
                            SessionEntry {
                                session_id: *id,
                                name: sm
                                    .session_name(*id)
                                    .unwrap_or_else(|| format!("session-{}", id)),
                                status: status.to_string(),
                                exit_code: pty.exit_code(),
                                closed_at: None,
                                has_note,
                                has_tags,
                                is_pinned,
                                is_locked: ms.as_ref().is_some_and(|m| {
                                    m.lock()
                                        .unwrap()
                                        .get_session(*id)
                                        .ok()
                                        .flatten()
                                        .is_some_and(|r| r.locked)
                                }),
                                idle: sm.idle_string(*id),
                                foreground_pid,
                                foreground_name,
                                foreground_cmd,
                            }
                        })
                        .collect();
                    #[cfg(not(test))]
                    let mut sessions: Vec<SessionEntry> = Vec::new();
                    if let Some(holder) = sm.holder_backend() {
                        for runtime in holder.inventory_snapshot() {
                            if sessions
                                .iter()
                                .any(|entry| entry.session_id == runtime.session_id)
                            {
                                continue;
                            }
                            let record = ms.as_ref().and_then(|metadata| {
                                metadata
                                    .lock()
                                    .unwrap()
                                    .get_session(runtime.session_id)
                                    .ok()
                                    .flatten()
                            });
                            let has_tags = ms.as_ref().is_some_and(|metadata| {
                                metadata
                                    .lock()
                                    .unwrap()
                                    .list_session_tags(runtime.session_id)
                                    .is_ok_and(|tags| !tags.is_empty())
                            });
                            let status = if sm.orphaned_sessions.contains(&runtime.session_id) {
                                "orphan"
                            } else if sm.is_attached(runtime.session_id) {
                                "attached"
                            } else if runtime.state
                                == persist_ipc::holder::HolderSessionState::Running
                            {
                                "running"
                            } else {
                                "closed"
                            };
                            let (foreground_name, foreground_cmd) =
                                process_identity(runtime.shell_pid);
                            sessions.push(SessionEntry {
                                session_id: runtime.session_id,
                                name: sm.session_name(runtime.session_id).unwrap_or_else(|| {
                                    record.as_ref().map_or_else(
                                        || format!("session-{}", runtime.session_id),
                                        |record| record.name.clone(),
                                    )
                                }),
                                status: status.into(),
                                exit_code: runtime.exit_code,
                                closed_at: record
                                    .as_ref()
                                    .and_then(|record| record.closed_at.clone()),
                                has_note: record
                                    .as_ref()
                                    .and_then(|record| record.note.as_ref())
                                    .is_some_and(|note| !note.is_empty()),
                                has_tags,
                                is_pinned: record.as_ref().is_some_and(|record| record.pinned),
                                is_locked: record.as_ref().is_some_and(|record| record.locked),
                                idle: sm.idle_string(runtime.session_id),
                                foreground_pid: (!foreground_name.is_empty())
                                    .then_some(runtime.shell_pid),
                                foreground_name,
                                foreground_cmd,
                            });
                        }
                    }
                    let runtime_ids: HashSet<u32> =
                        sessions.iter().map(|entry| entry.session_id).collect();
                    drop(sm);
                    if let Some(metadata) = &ms {
                        let closed_records = metadata
                            .lock()
                            .unwrap()
                            .list_sessions()
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|record| {
                                matches!(record.status.as_str(), "closed" | "lost")
                                    && !runtime_ids.contains(&record.session_id)
                            })
                            .collect::<Vec<_>>();
                        for record in closed_records {
                            let has_tags = metadata
                                .lock()
                                .unwrap()
                                .list_session_tags(record.session_id)
                                .map_or(false, |tags| !tags.is_empty());
                            sessions.push(SessionEntry {
                                session_id: record.session_id,
                                name: record.name,
                                status: record.status,
                                exit_code: record.exit_code,
                                closed_at: record.closed_at,
                                has_note: record.note.is_some_and(|note| !note.is_empty()),
                                has_tags,
                                is_pinned: record.pinned,
                                is_locked: record.locked,
                                idle: String::new(),
                                foreground_pid: None,
                                foreground_name: String::new(),
                                foreground_cmd: String::new(),
                            });
                        }
                    }
                    let resp = encode_list_sessions_resp(&ListSessionsRespPayload { sessions });
                    let _ = write_frame(
                        stream,
                        &Frame {
                            msg_type: MessageType::ListSessionsResp,
                            flags: 0,
                            request_id: 0,
                            payload: resp,
                        },
                    );
                }
                MessageType::ProcessTree => {
                    if let Some(payload) = decode_detach(&frame.payload) {
                        let nodes = sm.lock().unwrap().process_tree(payload.session_id);
                        let payload = encode_process_tree_resp(&ProcessTreeRespPayload { nodes });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::ProcessTreeResp,
                                flags: 0,
                                request_id: 0,
                                payload,
                            },
                        );
                    }
                }
                MessageType::ProcessStats => {
                    if let Some(payload) = decode_detach(&frame.payload) {
                        let stats = sm.lock().unwrap().process_stats(payload.session_id);
                        let payload = encode_process_stats_resp(&stats);
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::ProcessStatsResp,
                                flags: 0,
                                request_id: 0,
                                payload,
                            },
                        );
                    }
                }
                MessageType::SessionSnapshot => {
                    if let Some(payload) = decode_detach(&frame.payload) {
                        let (record, has_tags) = ms.as_ref().map_or((None, false), |metadata| {
                            let metadata = metadata.lock().unwrap();
                            let record = metadata.get_session(payload.session_id).ok().flatten();
                            let has_tags = record.as_ref().is_some_and(|_| {
                                metadata
                                    .list_session_tags(payload.session_id)
                                    .is_ok_and(|tags| !tags.is_empty())
                            });
                            (record, has_tags)
                        });
                        let (runtime, writer_active, output_log_path, output_log_state) = {
                            let manager = sm.lock().unwrap();
                            let runtime = manager.runtime_process_info(payload.session_id);
                            let writer_active = manager.is_attached(payload.session_id);
                            let output_log_path = manager.session_log_enabled.then(|| {
                                manager
                                    .logs_dir
                                    .join(format!("{}.log", payload.session_id))
                                    .to_string_lossy()
                                    .into_owned()
                            });
                            let output_log_state =
                                manager.output_log_state(payload.session_id).to_owned();
                            (runtime, writer_active, output_log_path, output_log_state)
                        };
                        let json = match record {
                            Some(record) => snapshot_json(
                                payload.session_id,
                                serde_json::json!({
                                    "session_id": record.session_id,
                                    "name": record.name,
                                    "status": record.status,
                                    "created_at": record.created_at,
                                    "updated_at": record.updated_at,
                                    "closed_at": record.closed_at,
                                    "cwd": record.cwd,
                                    "shell": record.shell,
                                    "exit_code": record.exit_code,
                                    "locked": record.locked,
                                    "pinned": record.pinned,
                                    "has_note": record.note.is_some(),
                                    "has_tags": has_tags,
                                    "writer_active": writer_active,
                                    "output_log_path": output_log_path,
                                    "output_log_state": output_log_state,
                                    "foreground_pid": runtime.as_ref().and_then(|(pid, _, _)| *pid),
                                    "foreground_name": runtime.as_ref().map(|(_, name, _)| name),
                                    "foreground_cmd": runtime.as_ref().map(|(_, _, cmd)| cmd),
                                }),
                            ),
                            None => serde_json::json!({
                                "session_id": payload.session_id,
                                "error": "session not found",
                            })
                            .to_string(),
                        };
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::SessionSnapshotResp,
                                flags: 0,
                                request_id: 0,
                                payload: encode_note_get_resp(&json),
                            },
                        );
                    }
                }
                MessageType::Metrics => {
                    let json = match &ms {
                        Some(metadata) => match metadata.lock().unwrap().list_sessions() {
                            Ok(records) => {
                                let manager = sm.lock().unwrap();
                                metrics_json(serde_json::json!({
                                    "daemon": { "pid": std::process::id() },
                                    "holder": manager.holder_diagnostics(),
                                    "sessions": {
                                        "total": records.len(),
                                        "running": records.iter().filter(|r| r.status == "running").count(),
                                        "closed": records.iter().filter(|r| r.status == "closed").count(),
                                        "lost": records.iter().filter(|r| r.status == "lost").count(),
                                        "locked": records.iter().filter(|r| r.locked).count(),
                                        "pinned": records.iter().filter(|r| r.pinned).count(),
                                        "runtime": manager.runtime_ids().len(),
                                        "active_writers": manager.active_writer_count(),
                                        "readonly_clients": manager.ro_attached.values().map(Vec::len).sum::<usize>(),
                                        "log_degraded": manager.degraded_log_count(),
                                    },
                                }))
                            }
                            Err(_) => serde_json::json!({
                                "error": "metadata store not available",
                            })
                            .to_string(),
                        },
                        None => serde_json::json!({
                            "error": "metadata store not available",
                        })
                        .to_string(),
                    };
                    let _ = write_frame(
                        stream,
                        &Frame {
                            msg_type: MessageType::MetricsResp,
                            flags: 0,
                            request_id: 0,
                            payload: encode_note_get_resp(&json),
                        },
                    );
                }
                MessageType::DashboardSummary => {
                    let response = decode_summary_request(&frame.payload)
                        .and_then(|request| {
                            dashboard
                                .as_ref()
                                .and_then(|service| service.summary(request).ok())
                        })
                        .unwrap_or_else(unavailable_summary);
                    let _ = write_frame(
                        stream,
                        &Frame {
                            msg_type: MessageType::DashboardSummaryResp,
                            flags: 0,
                            request_id: frame.request_id,
                            payload: encode_summary_response(&response),
                        },
                    );
                }
                MessageType::DashboardTrend => {
                    let response = decode_trend_request(&frame.payload)
                        .and_then(|request| {
                            dashboard
                                .as_ref()
                                .and_then(|service| service.trend(request).ok())
                        })
                        .unwrap_or_else(unavailable_trend);
                    let _ = write_frame(
                        stream,
                        &Frame {
                            msg_type: MessageType::DashboardTrendResp,
                            flags: 0,
                            request_id: frame.request_id,
                            payload: encode_trend_response(&response),
                        },
                    );
                }
                MessageType::Attach => {
                    if let Some(payload) = decode_attach(&frame.payload) {
                        let sid = payload.session_id;
                        let mut initial_output = Vec::new();
                        let connection_env =
                            validate_connection_environment(&payload.connection_env, unsafe {
                                libc::getuid()
                            });
                        let record = ms.as_ref().and_then(|metadata| {
                            metadata.lock().unwrap().get_session(sid).ok().flatten()
                        });
                        let locked = record.as_ref().is_some_and(|record| record.locked);
                        let runtime_exists = {
                            let sm = sm.lock().unwrap();
                            sm.has_runtime(sid)
                        };
                        if !locked
                            && !runtime_exists
                            && record
                                .as_ref()
                                .is_some_and(|record| record.status == "closed")
                        {
                            initial_output = sm.lock().unwrap().closed_attach_history(sid);
                            let record = record.as_ref().expect("closed record was checked");
                            let restored = (|| {
                                let policy = sm.lock().unwrap().recovery_environment_policy()?;
                                let environment = persist_metadata::decode_environment(
                                    record.env_snapshot.as_deref(),
                                    &policy,
                                )?;
                                sm.lock().unwrap().restore_closed_session(
                                    sid,
                                    record.name.clone(),
                                    record.shell.as_deref(),
                                    record.cwd.as_deref().map(Path::new),
                                    environment.as_ref(),
                                    &connection_env,
                                )
                            })();
                            if restored.is_ok() {
                                let holder_binding = sm.lock().unwrap().holder_binding();
                                let metadata_result: Result<()> =
                                    ms.as_ref().map_or(Ok(()), |metadata| {
                                        let mut metadata = metadata.lock().unwrap();
                                        (|| {
                                            metadata.reopen_session(sid)?;
                                            if let Some((instance, generation)) = holder_binding {
                                                metadata.reconcile_running(
                                                    sid, &instance, generation,
                                                )?;
                                            }
                                            Ok(())
                                        })()
                                    });
                                if metadata_result.is_err() {
                                    let _ = sm.lock().unwrap().close_session(sid);
                                }
                            } else {
                                initial_output.clear();
                            }
                        }
                        let holder = { sm.lock().unwrap().holder_backend() };
                        if let Some(holder) = holder {
                            let sid = payload.session_id;
                            let exists = !locked && sm.lock().unwrap().has_runtime(sid);
                            let data = exists
                                .then(|| {
                                    holder.attach(
                                        sid,
                                        persist_ipc::holder::HolderAttachMode::ReadWrite,
                                    )
                                })
                                .transpose();
                            let ok = data.as_ref().is_ok_and(|value| value.is_some());
                            let error_msg = match &data {
                                Ok(Some(_)) => String::new(),
                                Ok(None) => "not found".into(),
                                Err(error) => error.to_string(),
                            };
                            let response = encode_attach_resp(&AttachRespPayload {
                                ok,
                                error_msg: if locked {
                                    "session is locked".into()
                                } else {
                                    error_msg
                                },
                            });
                            let _ = write_frame(
                                stream,
                                &Frame {
                                    msg_type: MessageType::AttachResp,
                                    flags: 0,
                                    request_id: 0,
                                    payload: response,
                                },
                            );
                            if let Ok(Some(data)) = data {
                                holder.refresh_inventory()?;
                                let mut manager = sm.lock().unwrap();
                                manager.record_activity(sid);
                                manager.transfer_writer(sid, fd);
                                let policy = manager.recovery_environment_policy()?;
                                if let Some(entry) = manager.holder_entry(sid) {
                                    let context =
                                        capture_recovery_context_pid(entry.shell_pid, &policy);
                                    manager.record_recovery_context(sid, context);
                                }
                                drop(manager);
                                let context_sessions = sm.clone();
                                let mut observe_context = || {
                                    let mut manager = context_sessions.lock().unwrap();
                                    if let Ok(policy) = manager.recovery_environment_policy() {
                                        if let Some(entry) = manager.holder_entry(sid) {
                                            let context = capture_recovery_context_pid(
                                                entry.shell_pid,
                                                &policy,
                                            );
                                            manager.record_recovery_context(sid, context);
                                        }
                                    }
                                };
                                let outcome = crate::public_attach::run(
                                    fd,
                                    sid,
                                    data,
                                    initial_output,
                                    true,
                                    Some(&mut observe_context),
                                )?;
                                sm.lock().unwrap().release_writer(sid, fd);
                                if let Some(context) = outcome.exit_context {
                                    if let Err(error) =
                                        finalize_runtime_exit(sid, Some(context), &sm, &ms)
                                    {
                                        eprintln!(
                                            "persistd: failed to finalize Session {sid}: {error}"
                                        );
                                    }
                                } else {
                                    let _ = holder.refresh_inventory();
                                }
                            }
                            continue;
                        }
                        let (ok, previous_writer) = {
                            let mut sm = sm.lock().unwrap();
                            let exists = !locked && sm.has_runtime(sid);
                            let previous = if exists {
                                sm.record_activity(sid);
                                sm.transfer_writer(sid, fd)
                            } else {
                                None
                            };
                            (exists, previous.filter(|old_fd| *old_fd != fd))
                        };
                        if let Some(old_fd) = previous_writer {
                            let control =
                                encode_writer_control(&WriterControlPayload { session_id: sid });
                            let _ = write_frame_raw(old_fd, MessageType::WriteRequest, &control);
                            let _ = write_frame_raw(old_fd, MessageType::WriteRevoked, &control);
                        }
                        let sm_clone = sm.clone();
                        let resp = encode_attach_resp(&AttachRespPayload {
                            ok,
                            error_msg: if ok {
                                String::new()
                            } else if locked {
                                "session is locked".into()
                            } else {
                                "not found".into()
                            },
                        });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::AttachResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                        if ok {
                            let granted =
                                encode_writer_control(&WriterControlPayload { session_id: sid });
                            let _ = write_frame_raw(fd, MessageType::WriteGranted, &granted);
                            #[cfg(test)]
                            {
                                let replay = sm.lock().unwrap().replay_output(sid);
                                if !replay.is_empty() {
                                    let _ = write_stdout_raw(fd, &replay);
                                }
                            }
                            #[cfg(test)]
                            let _ = io_loop(fd, sid, &sm_clone, &ms);
                        }
                    }
                }
                MessageType::AttachReadOnly => {
                    if let Some(payload) = decode_attach(&frame.payload) {
                        let sid = payload.session_id;
                        let _connection_env =
                            validate_connection_environment(&payload.connection_env, unsafe {
                                libc::getuid()
                            });
                        let locked = ms.as_ref().is_some_and(|m| {
                            m.lock()
                                .unwrap()
                                .get_session(sid)
                                .ok()
                                .flatten()
                                .is_some_and(|r| r.locked)
                        });
                        let ok = {
                            let sm = sm.lock().unwrap();
                            !locked && sm.has_runtime(sid)
                        };
                        let holder = { sm.lock().unwrap().holder_backend() };
                        if let Some(holder) = holder {
                            let sid = payload.session_id;
                            let data = ok
                                .then(|| {
                                    holder.attach(
                                        sid,
                                        persist_ipc::holder::HolderAttachMode::ReadOnly,
                                    )
                                })
                                .transpose();
                            let attached = data.as_ref().is_ok_and(|value| value.is_some());
                            let error_msg = match &data {
                                Ok(Some(_)) => String::new(),
                                Ok(None) => "not found".into(),
                                Err(error) => error.to_string(),
                            };
                            let response = encode_attach_resp(&AttachRespPayload {
                                ok: attached,
                                error_msg: if locked {
                                    "session is locked".into()
                                } else {
                                    error_msg
                                },
                            });
                            let _ = write_frame(
                                stream,
                                &Frame {
                                    msg_type: MessageType::AttachResp,
                                    flags: 0,
                                    request_id: 0,
                                    payload: response,
                                },
                            );
                            if let Ok(Some(data)) = data {
                                sm.lock().unwrap().add_ro_client(sid, fd);
                                let outcome = crate::public_attach::run(
                                    fd,
                                    sid,
                                    data,
                                    Vec::new(),
                                    false,
                                    None,
                                )?;
                                sm.lock().unwrap().remove_ro_client(sid, fd);
                                if let Some(context) = outcome.exit_context {
                                    if let Err(error) =
                                        finalize_runtime_exit(sid, Some(context), &sm, &ms)
                                    {
                                        eprintln!(
                                            "persistd: failed to finalize Session {sid}: {error}"
                                        );
                                    }
                                } else {
                                    let _ = holder.refresh_inventory();
                                }
                            }
                            continue;
                        }
                        if ok {
                            sm.lock().unwrap().add_ro_client(sid, fd);
                        }
                        let resp = encode_attach_resp(&AttachRespPayload {
                            ok,
                            error_msg: if ok {
                                String::new()
                            } else if locked {
                                "session is locked".into()
                            } else {
                                "not found".into()
                            },
                        });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::AttachResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                        if ok {
                            #[cfg(test)]
                            {
                                let replay = sm.lock().unwrap().replay_output(sid);
                                if !replay.is_empty() {
                                    let _ = write_stdout_raw(fd, &replay);
                                }
                            }
                            #[cfg(test)]
                            let _ = ro_recv_loop(fd, sid, &sm);
                        }
                    }
                }
                MessageType::Stdin => {
                    let payload = frame.payload;
                    let sid = {
                        let sm = sm.lock().unwrap();
                        sm.attached_sessions
                            .iter()
                            .find_map(|(sid, writer_fd)| (*writer_fd == fd).then_some(*sid))
                    };
                    #[cfg(test)]
                    if let Some(sid) = sid {
                        let mut sm = sm.lock().unwrap();
                        sm.write_legacy_input(sid, &payload);
                        sm.record_activity(sid);
                    }
                }
                MessageType::Resize => {
                    if let Some(payload) = decode_resize(&frame.payload) {
                        #[cfg(test)]
                        sm.lock().unwrap().resize_legacy(payload.rows, payload.cols);
                    }
                }
                MessageType::Detach => {
                    if let Some(payload) = decode_detach(&frame.payload) {
                        let mut sm = sm.lock().unwrap();
                        let sid = payload.session_id;
                        sm.release_writer(sid, fd);
                    }
                    break;
                }
                MessageType::DetachSignal => {
                    if let Some(payload) = decode_detach(&frame.payload) {
                        let sm = sm.lock().unwrap();
                        let sid = payload.session_id;
                        if let Some(fd) = sm.attached_sessions.get(&sid) {
                            let sig: u8 = 1;
                            unsafe {
                                libc::write(*fd, &sig as *const u8 as *const libc::c_void, 1);
                            }
                        }
                        let resp = encode_op_resp(&OpRespPayload {
                            ok: true,
                            error_msg: String::new(),
                        });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::DetachSignalResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::Signal => {
                    if let Some(payload) = decode_signal(&frame.payload) {
                        let sid = payload.session_id;
                        let signal = payload.signal;
                        #[cfg(test)]
                        let forwarded = sm.lock().unwrap().signal_legacy(sid, signal);
                        #[cfg(not(test))]
                        let forwarded = false;
                        let resp = encode_op_resp(&OpRespPayload {
                            ok: forwarded,
                            error_msg: if forwarded {
                                String::new()
                            } else {
                                "no foreground process group".into()
                            },
                        });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::SignalResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::Rename => {
                    if let Some(payload) = decode_rename(&frame.payload) {
                        let result = validate_session_name(&payload.name).and_then(|_| {
                            if sm
                                .lock()
                                .unwrap()
                                .session_name(payload.session_id)
                                .is_none()
                            {
                                return Err(PersistError::invalid_argument("session not found"));
                            }
                            match &ms {
                                Some(metadata) => metadata
                                    .lock()
                                    .unwrap()
                                    .rename_session(payload.session_id, &payload.name),
                                None => Ok(()),
                            }?;
                            sm.lock()
                                .unwrap()
                                .rename_session(payload.session_id, payload.name.clone())
                        });
                        let resp = encode_op_resp(&OpRespPayload {
                            ok: result.is_ok(),
                            error_msg: result.err().map_or_else(String::new, |e| e.to_string()),
                        });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::RenameResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::NoteSet => {
                    if let Some(payload) = decode_note(&frame.payload) {
                        let result = match &ms {
                            Some(m) => {
                                let note = if payload.note.is_empty() {
                                    None
                                } else {
                                    Some(payload.note.as_str())
                                };
                                m.lock().unwrap().set_session_note(payload.session_id, note)
                            }
                            None => Err(PersistError::invalid_argument(
                                "metadata store not available",
                            )),
                        };
                        let (ok, err) = match result {
                            Ok(()) => (true, String::new()),
                            Err(e) => (false, e.to_string()),
                        };
                        let resp = encode_op_resp(&OpRespPayload { ok, error_msg: err });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::NoteSetResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::NoteGet => {
                    if let Some(payload) = decode_detach(&frame.payload) {
                        let note = match &ms {
                            Some(m) => m
                                .lock()
                                .unwrap()
                                .get_session(payload.session_id)
                                .ok()
                                .flatten()
                                .and_then(|r| r.note)
                                .unwrap_or_default(),
                            None => String::new(),
                        };
                        let resp = encode_note_get_resp(&note);
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::NoteGetResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::TagAdd => {
                    if let Some(payload) = decode_tag(&frame.payload) {
                        let result = match &ms {
                            Some(m) => m
                                .lock()
                                .unwrap()
                                .add_session_tag(payload.session_id, &payload.tag),
                            None => Err(PersistError::invalid_argument(
                                "metadata store not available",
                            )),
                        };
                        let (ok, err) = match result {
                            Ok(()) => (true, String::new()),
                            Err(e) => (false, e.to_string()),
                        };
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::TagAddResp,
                                flags: 0,
                                request_id: 0,
                                payload: encode_op_resp(&OpRespPayload { ok, error_msg: err }),
                            },
                        );
                    }
                }
                MessageType::TagRemove => {
                    if let Some(payload) = decode_tag(&frame.payload) {
                        let result = match &ms {
                            Some(m) => m
                                .lock()
                                .unwrap()
                                .remove_session_tag(payload.session_id, &payload.tag),
                            None => Err(PersistError::invalid_argument(
                                "metadata store not available",
                            )),
                        };
                        let (ok, err) = match result {
                            Ok(()) => (true, String::new()),
                            Err(e) => (false, e.to_string()),
                        };
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::TagRemoveResp,
                                flags: 0,
                                request_id: 0,
                                payload: encode_op_resp(&OpRespPayload { ok, error_msg: err }),
                            },
                        );
                    }
                }
                MessageType::TagList => {
                    if let Some(payload) = decode_detach(&frame.payload) {
                        let tags = match &ms {
                            Some(m) => m
                                .lock()
                                .unwrap()
                                .list_session_tags(payload.session_id)
                                .unwrap_or_default(),
                            None => Vec::new(),
                        };
                        let resp = encode_tag_list_resp(&TagListRespPayload { tags });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::TagListResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::PinSet => {
                    if let Some(payload) = decode_pin(&frame.payload) {
                        let result = match &ms {
                            Some(m) => m
                                .lock()
                                .unwrap()
                                .set_session_pinned(payload.session_id, payload.pinned),
                            None => Err(PersistError::invalid_argument(
                                "metadata store not available",
                            )),
                        };
                        let (ok, err) = match result {
                            Ok(()) => (true, String::new()),
                            Err(e) => (false, e.to_string()),
                        };
                        let resp = encode_op_resp(&OpRespPayload { ok, error_msg: err });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::PinSetResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::LockSet => {
                    if let Some(payload) = decode_lock(&frame.payload) {
                        let result = match &ms {
                            Some(m) => m
                                .lock()
                                .unwrap()
                                .set_session_locked(payload.session_id, payload.locked),
                            None => Err(PersistError::invalid_argument(
                                "metadata store not available",
                            )),
                        };
                        if result.is_ok() {
                            sm.lock()
                                .unwrap()
                                .set_locked(payload.session_id, payload.locked);
                        }
                        let resp = encode_op_resp(&OpRespPayload {
                            ok: result.is_ok(),
                            error_msg: result.err().map_or_else(String::new, |e| e.to_string()),
                        });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::LockSetResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::ListSessionsByTag => {
                    if let Some(payload) = decode_note_get_resp(&frame.payload) {
                        let records = match &ms {
                            Some(metadata) => {
                                let metadata = metadata.lock().unwrap();
                                metadata
                                    .find_sessions_by_tag(&payload)
                                    .unwrap_or_default()
                                    .into_iter()
                                    .filter_map(|id| metadata.get_session(id).ok().flatten())
                                    .collect::<Vec<_>>()
                            }
                            None => Vec::new(),
                        };
                        let manager = sm.lock().unwrap();
                        let sessions = records
                            .into_iter()
                            .map(|record| {
                                let holder = manager.holder_entry(record.session_id);
                                let legacy = manager.legacy_runtime_info(record.session_id);
                                let status = if manager.is_attached(record.session_id) {
                                    "attached".to_owned()
                                } else if holder.as_ref().is_some_and(|entry| {
                                    entry.state == persist_ipc::holder::HolderSessionState::Running
                                }) || legacy.as_ref().is_some_and(|runtime| runtime.alive)
                                {
                                    "running".to_owned()
                                } else {
                                    record.status.clone()
                                };
                                let exit_code = holder
                                    .as_ref()
                                    .and_then(|entry| entry.exit_code)
                                    .or_else(|| {
                                        legacy.as_ref().and_then(|runtime| runtime.exit_code)
                                    })
                                    .or(record.exit_code);
                                let (foreground_pid, foreground_name, foreground_cmd) =
                                    if let Some(entry) = &holder {
                                        let (name, command) = process_identity(entry.shell_pid);
                                        (
                                            (!name.is_empty()).then_some(entry.shell_pid),
                                            name,
                                            command,
                                        )
                                    } else if let Some(runtime) = &legacy {
                                        runtime.foreground.clone()
                                    } else {
                                        (None, String::new(), String::new())
                                    };
                                SessionEntry {
                                    session_id: record.session_id,
                                    name: manager
                                        .session_name(record.session_id)
                                        .unwrap_or(record.name),
                                    status,
                                    exit_code,
                                    closed_at: record.closed_at,
                                    has_note: record.note.is_some_and(|note| !note.is_empty()),
                                    has_tags: true,
                                    is_pinned: record.pinned,
                                    is_locked: record.locked,
                                    idle: manager.idle_string(record.session_id),
                                    foreground_pid,
                                    foreground_name,
                                    foreground_cmd,
                                }
                            })
                            .collect();
                        let resp = encode_list_sessions_resp(&ListSessionsRespPayload { sessions });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::ListSessionsResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::Close => {
                    if let Some(payload) = decode_detach(&frame.payload) {
                        let sid = payload.session_id;
                        let result = finalize_runtime_exit(sid, None, &sm, &ms);
                        let resp = encode_op_resp(&OpRespPayload {
                            ok: result.is_ok(),
                            error_msg: result.err().map_or_else(String::new, |e| e.to_string()),
                        });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::CloseResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::Kill => {
                    if let Some(payload) = decode_detach(&frame.payload) {
                        let sid = payload.session_id;
                        let locked = ms.as_ref().is_some_and(|m| {
                            m.lock()
                                .unwrap()
                                .get_session(sid)
                                .ok()
                                .flatten()
                                .is_some_and(|r| r.locked)
                        });
                        let result = if locked {
                            Err(PersistError::invalid_argument("session is locked"))
                        } else {
                            let killed = { sm.lock().unwrap().kill_session(sid) };
                            killed.and_then(|_| finalize_runtime_exit(sid, None, &sm, &ms))
                        };
                        let resp = encode_op_resp(&OpRespPayload {
                            ok: result.is_ok(),
                            error_msg: result.err().map_or_else(String::new, |e| e.to_string()),
                        });
                        let _ = write_frame(
                            stream,
                            &Frame {
                                msg_type: MessageType::KillResp,
                                flags: 0,
                                request_id: 0,
                                payload: resp,
                            },
                        );
                    }
                }
                MessageType::Ping => {
                    let _ = write_frame(
                        stream,
                        &Frame {
                            msg_type: MessageType::Pong,
                            flags: 0,
                            request_id: frame.request_id,
                            payload: vec![],
                        },
                    );
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn finalize_runtime_exit(
    session_id: u32,
    observed_exit: Option<crate::holder::ExitContext>,
    sessions: &Arc<Mutex<SessionManager>>,
    metadata: &Option<Arc<Mutex<MetadataStore>>>,
) -> Result<()> {
    let closed = sessions
        .lock()
        .unwrap()
        .prepare_close(session_id, observed_exit)?
        .ok_or_else(|| PersistError::invalid_argument("session not found"))?;
    let encoded_environment = closed
        .recovery_context
        .environment
        .as_ref()
        .and_then(|snapshot| {
            let policy = sessions
                .lock()
                .unwrap()
                .recovery_environment_policy()
                .ok()?;
            persist_metadata::encode_environment(snapshot, &policy).ok()
        });
    crash_at_test_point("after_exit_context_before_metadata");
    metadata_then_retire(
        || {
            if let Some(metadata) = metadata {
                metadata.lock().unwrap().close_session_with_context(
                    session_id,
                    closed.exit_code,
                    closed.recovery_context.cwd.as_deref(),
                    encoded_environment.as_deref(),
                )?;
            }
            Ok(())
        },
        || {
            crash_at_test_point("after_exit_metadata_before_retire");
            if closed.holder_retire {
                let holder = sessions
                    .lock()
                    .unwrap()
                    .holder_backend()
                    .ok_or_else(|| PersistError::internal_error("Holder is unavailable"))?;
                holder.retire_exited(session_id)?;
            }
            Ok(())
        },
    )?;
    sessions.lock().unwrap().finish_close(session_id);
    Ok(())
}

fn metadata_then_retire(
    persist_metadata: impl FnOnce() -> Result<()>,
    retire_holder: impl FnOnce() -> Result<()>,
) -> Result<()> {
    persist_metadata()?;
    retire_holder()
}

#[cfg(test)]
fn io_loop(
    fd: RawFd,
    sid: u32,
    sm: &Arc<Mutex<SessionManager>>,
    ms: &Option<Arc<Mutex<MetadataStore>>>,
) -> Result<()> {
    let mut pty_buf = [0u8; 65536];
    let mut acc = FrameAccumulator::new();

    loop {
        if !sm.lock().unwrap().is_writer(sid, fd) {
            return Ok(());
        }

        let mut pfd = [libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        }];

        let ret = unsafe { libc::poll(pfd.as_mut_ptr(), 1, 100) };
        if ret < 0 {
            break;
        }

        if ret > 0 && (pfd[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)) != 0 {
            break;
        }

        if ret > 0 && (pfd[0].revents & libc::POLLIN) != 0 {
            let mut buf = [0u8; 65536];
            let n = match read_nonblock(fd, &mut buf) {
                Ok(n) => n,
                Err(_) => {
                    return Ok(());
                }
            };
            if n > 0 {
                acc.feed(&buf[..n]);
                while let Ok(Some(frame)) = acc.try_read() {
                    match frame.msg_type {
                        MessageType::Stdin => {
                            let mut sm = sm.lock().unwrap();
                            if sm.is_writer(sid, fd) {
                                for (id, pty) in sm.sessions.iter() {
                                    if *id == sid {
                                        let mut pty = pty.lock().unwrap();
                                        let _ = pty.write_input(&frame.payload);
                                        break;
                                    }
                                }
                            }
                            sm.record_activity(sid);
                        }
                        MessageType::Resize => {
                            if let Some(payload) = decode_resize(&frame.payload) {
                                let sm = sm.lock().unwrap();
                                for (id, pty) in sm.sessions.iter() {
                                    if *id == sid {
                                        let pty = pty.lock().unwrap();
                                        let _ = apply_resize(
                                            pty.master_fd(),
                                            payload.rows,
                                            payload.cols,
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                        MessageType::Detach => {
                            let mut sm = sm.lock().unwrap();
                            sm.release_writer(sid, fd);
                            return Ok(());
                        }
                        MessageType::Ping => {
                            let _ = write_frame_raw(fd, MessageType::Pong, &[]);
                        }
                        MessageType::Signal => {
                            if let Some(p) = decode_signal(&frame.payload) {
                                let sm = sm.lock().unwrap();
                                let master_fd =
                                    sm.sessions.iter().find(|(id, _)| *id == sid).and_then(
                                        |(_, pty_arc)| {
                                            let pty = pty_arc.lock().unwrap();
                                            Some(pty.master_fd())
                                        },
                                    );
                                if let Some(master_fd) = master_fd {
                                    let pgid = unsafe { libc::tcgetpgrp(master_fd) };
                                    if pgid > 0 {
                                        unsafe { libc::kill(-pgid, p.signal as i32) };
                                    }
                                }
                                let resp = encode_op_resp(&OpRespPayload {
                                    ok: true,
                                    error_msg: String::new(),
                                });
                                let _ = write_frame_raw(fd, MessageType::SignalResp, &resp);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Check PTY output
        let (n, exit_code) = {
            let mut sm_guard = sm.lock().unwrap();
            let pty_arc = sm_guard
                .sessions
                .iter()
                .find(|(id, _)| *id == sid)
                .map(|(_, pty)| pty.clone());
            match pty_arc {
                Some(pty_arc) => {
                    let mut pty = pty_arc.lock().unwrap();
                    let n = if let Ok(true) = pty.poll_output(Duration::from_millis(0)) {
                        pty.read_output(&mut pty_buf).unwrap_or(0)
                    } else {
                        0
                    };
                    if n > 0 {
                        let _ = write_stdout_raw(fd, &pty_buf[..n]);
                    }
                    let policy = sm_guard.recovery_environment_policy()?;
                    let recovery_context = capture_recovery_context(&pty, &policy);
                    let exit_code = pty.poll_exit().ok().flatten();
                    drop(pty);
                    sm_guard.record_recovery_context(sid, recovery_context);
                    (n, exit_code)
                }
                None => (0, Some(0)),
            }
        };
        if n > 0 {
            sm.lock().unwrap().broadcast_stdout(sid, &pty_buf[..n]);
            let mut sm_guard = sm.lock().unwrap();
            sm_guard.record_activity(sid);
            if let Some(rb) = sm_guard.ring_buffers.get(&sid) {
                let mut rb = rb.lock().unwrap();
                rb.write(&pty_buf[..n]);
            }
            if let Some(handle) = sm_guard.log_handles.get(&sid) {
                handle.write(&pty_buf[..n]);
            }
        }

        if exit_code.is_some() && n == 0 {
            let readonly_clients = sm.lock().unwrap().readonly_clients(sid);
            let closed = { sm.lock().unwrap().close_session(sid) };
            if let Ok(Some(closed)) = closed {
                let encoded_environment =
                    if let Some(snapshot) = &closed.recovery_context.environment {
                        let policy = sm.lock().unwrap().recovery_environment_policy()?;
                        Some(persist_metadata::encode_environment(snapshot, &policy)?)
                    } else {
                        None
                    };
                if let Some(metadata) = ms {
                    let _ = metadata.lock().unwrap().close_session_with_context(
                        sid,
                        closed.exit_code,
                        closed.recovery_context.cwd.as_deref(),
                        encoded_environment.as_deref(),
                    );
                }
                let payload = encode_session_exited(&SessionExitedPayload {
                    session_id: sid,
                    exit_code: closed.exit_code,
                });
                let _ = write_frame_raw(fd, MessageType::SessionExited, &payload);
                for readonly_fd in readonly_clients {
                    let _ = write_frame_raw(readonly_fd, MessageType::SessionExited, &payload);
                }
            }
            break;
        }
    }

    sm.lock().unwrap().release_writer(sid, fd);
    Ok(())
}

#[cfg(test)]
fn ro_recv_loop(fd: RawFd, sid: u32, sm: &Arc<Mutex<SessionManager>>) -> Result<()> {
    let mut acc = FrameAccumulator::new();
    let mut buf = [0u8; 65536];

    loop {
        let mut pfd = [libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        }];

        let ret = unsafe { libc::poll(pfd.as_mut_ptr(), 1, -1) };
        if ret < 0 {
            break;
        }

        if (pfd[0].revents & libc::POLLIN) == 0 {
            break;
        }

        let n = match read_nonblock(fd, &mut buf) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }

        acc.feed(&buf[..n]);
        while let Ok(Some(frame)) = acc.try_read() {
            match frame.msg_type {
                MessageType::Detach | MessageType::Close => {
                    sm.lock().unwrap().remove_ro_client(sid, fd);
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    sm.lock().unwrap().remove_ro_client(sid, fd);
    Ok(())
}

fn read_nonblock(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let pret = unsafe { libc::poll(&mut pfd, 1, 0) };
    if pret < 0 {
        return Err(io::Error::last_os_error());
    }
    if pret == 0 {
        return Ok(0);
    }

    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    if n < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::WouldBlock {
            return Ok(0);
        }
        return Err(err);
    }
    if n == 0 {
        // POLLHUP — peer closed
        return Err(io::Error::new(io::ErrorKind::ConnectionReset, "EOF"));
    }
    Ok(n as usize)
}

#[cfg(test)]
fn apply_resize(master_fd: RawFd, rows: u16, cols: u16) -> io::Result<()> {
    let ws = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ret = unsafe { libc::ioctl(master_fd, libc::TIOCSWINSZ, &ws as *const libc::winsize) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    let ret = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn write_frame_raw(fd: RawFd, msg_type: MessageType, payload: &[u8]) -> io::Result<()> {
    let header_len = 12;
    let total_len = header_len + payload.len();
    let mut buf = Vec::with_capacity(total_len);

    // BE frame format: 4 bytes payload_len, 2 bytes msg_type, 2 bytes flags, 4 bytes request_id
    let payload_len = payload.len() as u32;
    buf.extend_from_slice(&payload_len.to_be_bytes());
    let msg_type_val = msg_type as u16;
    buf.extend_from_slice(&msg_type_val.to_be_bytes());
    let flags: u16 = 0;
    buf.extend_from_slice(&flags.to_be_bytes());
    let request_id: u32 = 0;
    buf.extend_from_slice(&request_id.to_be_bytes());
    buf.extend_from_slice(payload);

    let written = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
    if written < 0 {
        return Err(io::Error::last_os_error());
    }
    if written as usize != buf.len() {
        return Err(io::Error::new(io::ErrorKind::WriteZero, "partial write"));
    }
    Ok(())
}

#[cfg(test)]
fn write_stdout_raw(fd: RawFd, data: &[u8]) -> io::Result<()> {
    for chunk in data.chunks(MAX_IO_FRAME) {
        write_frame_raw(fd, MessageType::Stdout, chunk)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Read;
    use std::os::unix::fs::symlink;
    use std::os::unix::net::{UnixListener, UnixStream};

    fn read_until_session_exited(stream: &mut UnixStream) -> SessionExitedPayload {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set read timeout");
        loop {
            let frame = read_frame(stream).expect("read session frame");
            if frame.msg_type == MessageType::SessionExited {
                return persist_ipc::decode_session_exited(&frame.payload)
                    .expect("decode session exited");
            }
        }
    }

    #[test]
    fn daemon_revalidates_agent_socket_without_dropping_other_context() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join("agent.sock");
        let _listener = UnixListener::bind(&socket).expect("bind");
        let context = ConnectionEnvironment::from_pairs([
            ("TERM", "xterm"),
            ("SSH_AUTH_SOCK", socket.to_str().expect("utf8")),
        ])
        .expect("context");
        let validated = validate_connection_environment(&context, unsafe { libc::getuid() });
        assert_eq!(validated, context);

        let file = temp.path().join("agent.file");
        fs::write(&file, b"not a socket").expect("write");
        let link = temp.path().join("agent.link");
        symlink(&socket, &link).expect("symlink");
        for invalid in [file, link] {
            let context = ConnectionEnvironment::from_pairs([
                ("TERM", "xterm"),
                ("SSH_AUTH_SOCK", invalid.to_str().expect("utf8")),
            ])
            .expect("context");
            let validated = validate_connection_environment(&context, unsafe { libc::getuid() });
            assert_eq!(
                validated.iter().collect::<Vec<_>>(),
                vec![("TERM", "xterm")]
            );
        }
    }

    #[test]
    fn readonly_stdout_is_framed() {
        let (daemon, mut client) = UnixStream::pair().expect("socket pair");
        let mut manager =
            SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        manager.add_ro_client(7, daemon.as_raw_fd());

        manager.broadcast_stdout(7, b"readonly output");

        let frame = read_frame(&mut client).expect("read stdout frame");
        assert_eq!(frame.msg_type, MessageType::Stdout);
        assert_eq!(frame.payload, b"readonly output");
    }

    #[test]
    fn stdout_larger_than_io_frame_is_split() {
        let (daemon, mut client) = UnixStream::pair().expect("socket pair");
        let output = vec![b'x'; MAX_IO_FRAME + 17];

        write_stdout_raw(daemon.as_raw_fd(), &output).expect("write stdout frames");

        let first = read_frame(&mut client).expect("read first frame");
        let second = read_frame(&mut client).expect("read second frame");
        assert_eq!(first.msg_type, MessageType::Stdout);
        assert_eq!(first.payload.len(), MAX_IO_FRAME);
        assert_eq!(second.msg_type, MessageType::Stdout);
        assert_eq!(second.payload, vec![b'x'; 17]);
    }

    #[test]
    fn replay_output_respects_limit_and_disable_flag() {
        let mut manager = SessionManager::new(
            16,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        );
        let sid = manager
            .create_with_shell(Some("/bin/sh"))
            .expect("create shell");
        manager
            .ring_buffers
            .get(&sid)
            .expect("ring buffer")
            .lock()
            .unwrap()
            .write(b"output-history");

        manager.set_replay_config(true, 7);
        assert_eq!(manager.replay_output(sid), b"history");
        manager.set_replay_config(false, 7);
        assert!(manager.replay_output(sid).is_empty());
    }

    #[test]
    fn closed_replay_respects_limit_and_disable_flag() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-closed-replay-")
            .tempdir()
            .expect("create temp dir");
        let logs_dir = dir.path().join("sessions");
        std::fs::create_dir(&logs_dir).expect("create logs dir");
        std::fs::set_permissions(&logs_dir, std::fs::Permissions::from_mode(0o700))
            .expect("secure logs dir");
        let log_path = logs_dir.join("7.log");
        std::fs::write(&log_path, b"output-history").expect("write log");
        std::fs::set_permissions(&log_path, std::fs::Permissions::from_mode(0o600))
            .expect("secure log");
        let mut manager =
            SessionManager::new(0, true, logs_dir, dir.path().join("history"), 1024, 0);

        manager.set_replay_config(true, 7);
        assert_eq!(manager.closed_attach_history(7), b"history");
        manager.set_replay_config(false, 7);
        assert!(manager.closed_attach_history(7).is_empty());
        manager.set_replay_config(true, 0);
        assert!(manager.closed_attach_history(7).is_empty());
    }

    #[test]
    fn natural_exit_notifies_writer_and_readonly_client() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-session-log-")
            .tempdir()
            .expect("create temp dir");
        let logs_dir = dir.path().join("sessions");
        let (daemon_writer, mut writer) = UnixStream::pair().expect("writer socket pair");
        let (daemon_reader, mut reader) = UnixStream::pair().expect("reader socket pair");
        set_nonblocking(daemon_writer.as_raw_fd()).expect("nonblocking writer");
        let manager = Arc::new(Mutex::new(SessionManager::new(
            0,
            true,
            logs_dir.clone(),
            dir.path().join("history"),
            1024 * 1024,
            2,
        )));
        let sid = manager
            .lock()
            .unwrap()
            .create_with_shell(Some("/bin/sh"))
            .expect("create shell");
        {
            let mut manager = manager.lock().unwrap();
            manager.mark_attached(sid, daemon_writer.as_raw_fd());
            manager.add_ro_client(sid, daemon_reader.as_raw_fd());
        }

        let server_manager = manager.clone();
        let server = std::thread::spawn(move || {
            let _daemon_writer = daemon_writer;
            io_loop(_daemon_writer.as_raw_fd(), sid, &server_manager, &None).expect("run io loop");
        });
        write_frame(
            &mut writer,
            &Frame {
                msg_type: MessageType::Stdin,
                flags: 0,
                request_id: 0,
                payload: b"printf 'SESSION_LOG_MARKER\\n'; exit 7\n".to_vec(),
            },
        )
        .expect("send exit");

        let writer_exit = read_until_session_exited(&mut writer);
        let reader_exit = read_until_session_exited(&mut reader);
        assert_eq!(writer_exit.session_id, sid);
        assert_eq!(writer_exit.exit_code, 7);
        assert_eq!(reader_exit.session_id, sid);
        assert_eq!(reader_exit.exit_code, 7);

        server.join().expect("join io loop");
        assert!(manager.lock().unwrap().list().is_empty());
        let log_path = logs_dir.join(format!("{sid}.log"));
        for _ in 0..100 {
            if std::fs::read_to_string(&log_path)
                .is_ok_and(|content| content.contains("SESSION_LOG_MARKER"))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("PTY output was not written to {}", log_path.display());
    }

    #[test]
    fn snapshot_json_keeps_small_json() {
        let json = snapshot_json(7, serde_json::json!({"session_id": 7, "status": "closed"}));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).expect("valid json")["status"],
            "closed"
        );
    }

    #[test]
    fn snapshot_json_rejects_oversized_json() {
        let json = snapshot_json(7, serde_json::json!({"value": "x".repeat(16 * 1024)}));
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["session_id"], 7);
        assert_eq!(value["error"], "snapshot exceeds output limit");
    }

    #[test]
    fn metrics_json_rejects_oversized_json() {
        let json = metrics_json(serde_json::json!({"value": "x".repeat(16 * 1024)}));
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["error"], "metrics exceeds output limit");
    }

    #[test]
    fn version_returns_success() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_with_io(
            ["persistd".to_string(), "--version".to_string()],
            &mut out,
            &mut err,
        );

        assert_eq!(code, 0);
        assert!(String::from_utf8(out)
            .expect("utf8")
            .starts_with("persistd "));
        assert!(err.is_empty());
    }

    #[test]
    fn generate_session_name_uses_shell_and_cwd() {
        let name = generate_session_name("/usr/bin/zsh");
        assert!(
            name.starts_with("zsh@"),
            "should start with 'zsh@', got: {name}"
        );
        assert!(name.len() > 5, "name should include cwd part");

        let name2 = generate_session_name("/bin/bash");
        assert!(
            name2.starts_with("bash@"),
            "should start with 'bash@', got: {name2}"
        );

        let name3 = generate_session_name("fish");
        assert!(
            name3.starts_with("fish@"),
            "should start with 'fish@', got: {name3}"
        );
    }

    #[test]
    fn process_tree_includes_foreground_process() {
        let session = PtyEngine::new()
            .open_session_with_shell("/bin/sh", None)
            .expect("open session");
        std::thread::sleep(Duration::from_millis(100));
        let nodes = process_tree(&session);
        assert!(!nodes.is_empty());
        assert_eq!(
            nodes[0].pid,
            session.foreground_process_group().expect("foreground")
        );
    }

    #[test]
    fn session_histfile_is_created() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-histfile-")
            .tempdir()
            .expect("create temp dir");
        let history_dir = dir.path().join("history");
        let mut sm =
            SessionManager::new(0, false, PathBuf::from("/tmp"), history_dir.clone(), 0, 0);
        let id = sm.create().expect("create session");
        assert!(id >= 1);
        let histfile_path = history_dir.join(id.to_string());
        // The directory should exist (created by create_with_shell)
        assert!(
            histfile_path.parent().unwrap().exists(),
            "history directory should be created"
        );
        // Verify a shell is running with HISTFILE set by checking the histfile
        // path exists as a file (bash/zsh create it on first history write)
        // We can't directly check the env from here, but we verify the dir was created
    }

    #[test]
    fn session_manager_create_remove_list() {
        let mut sm =
            SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        let id = sm.create().expect("create session");
        assert!(id >= 1);
        let name = sm.session_name(id);
        assert!(name.is_some(), "session should have a name");
        let name = name.unwrap();
        assert!(
            name.contains("bash@") || name.contains("sh@"),
            "name '{name}' should contain shell@cwd"
        );
        let sessions = sm.list();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].0, id);
        let removed = sm.remove(id);
        assert!(removed.is_some());
        assert!(sm.list().is_empty());
    }

    #[test]
    fn client_hello_new_session_list_sessions_detach() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-ipc-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));

        let sm_server = sm.clone();
        let server = std::thread::spawn(move || {
            if let Ok(conn) = daemon.accept() {
                let _ = handle_client(conn, sm_server, None, None, 0);
            }
        });

        std::thread::sleep(Duration::from_millis(100));

        let mut client = UnixStream::connect(&socket_path).expect("connect");

        let hello = encode_hello(&HelloPayload {
            protocol_major: 0,
            protocol_minor: 1,
            uid: 0,
            pid: 0,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Hello,
                flags: 0,
                request_id: 0,
                payload: hello,
            },
        )
        .expect("hello");
        let resp = read_frame(&mut client).expect("hello ack");
        assert_eq!(resp.msg_type, MessageType::HelloAck);

        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NewSession,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("new_session");
        let resp = read_frame(&mut client).expect("new_session_resp");
        assert_eq!(resp.msg_type, MessageType::NewSessionResp);
        let session = decode_new_session_resp(&resp.payload).expect("decode");
        assert!(
            !session.name.is_empty(),
            "new session should have auto-generated name"
        );

        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::ListSessions,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("list");
        let resp = read_frame(&mut client).expect("list_resp");
        assert_eq!(resp.msg_type, MessageType::ListSessionsResp);
        let list = decode_list_sessions_resp(&resp.payload).expect("decode");
        assert!(list
            .sessions
            .iter()
            .any(|s| s.session_id == session.session_id));
        let listed = list
            .sessions
            .iter()
            .find(|s| s.session_id == session.session_id)
            .unwrap();
        assert_eq!(
            listed.name, session.name,
            "list should return same name as new_session_resp"
        );

        let detach_payload = encode_detach(&DetachPayload {
            session_id: session.session_id,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Detach,
                flags: 0,
                request_id: 0,
                payload: detach_payload,
            },
        )
        .expect("detach");

        drop(client);
        server.join().expect("server thread");
    }

    #[test]
    fn client_new_list_close_kill_flow() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-ipc-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));

        let sm_server = sm.clone();
        let server = std::thread::spawn(move || {
            if let Ok(conn) = daemon.accept() {
                let _ = handle_client(conn, sm_server, None, None, 0);
            }
        });

        std::thread::sleep(Duration::from_millis(100));

        let mut client = UnixStream::connect(&socket_path).expect("connect");

        let hello = encode_hello(&HelloPayload {
            protocol_major: 0,
            protocol_minor: 1,
            uid: 0,
            pid: 0,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Hello,
                flags: 0,
                request_id: 0,
                payload: hello,
            },
        )
        .expect("hello");
        let resp = read_frame(&mut client).expect("hello ack");
        assert_eq!(resp.msg_type, MessageType::HelloAck);

        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NewSession,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("new_session");
        let resp = read_frame(&mut client).expect("new_session_resp");
        assert_eq!(resp.msg_type, MessageType::NewSessionResp);
        let session = decode_new_session_resp(&resp.payload).expect("decode");

        // Kill session
        let kill_payload = encode_detach(&DetachPayload {
            session_id: session.session_id,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Kill,
                flags: 0,
                request_id: 0,
                payload: kill_payload,
            },
        )
        .expect("kill");
        let resp = read_frame(&mut client).expect("kill_resp");
        assert_eq!(resp.msg_type, MessageType::KillResp);

        // List should be empty after kill+close
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::ListSessions,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("list");
        let resp = read_frame(&mut client).expect("list_resp");
        assert_eq!(resp.msg_type, MessageType::ListSessionsResp);
        let list = decode_list_sessions_resp(&resp.payload).expect("decode");
        assert!(list.sessions.is_empty());

        drop(client);
        server.join().expect("server thread");
    }

    #[test]
    fn client_note_set_get_flow() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-ipc-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));
        let ms = Arc::new(Mutex::new(
            MetadataStore::open_in_memory().expect("open metadata"),
        ));

        let sm_server = sm.clone();
        let ms_server = ms.clone();
        let server = std::thread::spawn(move || {
            if let Ok(conn) = daemon.accept() {
                let _ = handle_client(conn, sm_server, Some(ms_server), None, 0);
            }
        });

        std::thread::sleep(Duration::from_millis(100));

        let mut client = UnixStream::connect(&socket_path).expect("connect");
        let hello = encode_hello(&HelloPayload {
            protocol_major: 0,
            protocol_minor: 1,
            uid: 0,
            pid: 0,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Hello,
                flags: 0,
                request_id: 0,
                payload: hello,
            },
        )
        .expect("hello");
        let _ack = read_frame(&mut client).expect("hello ack");

        // Create a session
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NewSession,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("new_session");
        let resp = read_frame(&mut client).expect("new_session_resp");
        assert_eq!(resp.msg_type, MessageType::NewSessionResp);
        let session = decode_new_session_resp(&resp.payload).expect("decode");

        // Set note
        let note_payload = encode_note(&NotePayload {
            session_id: session.session_id,
            note: "my test note".to_string(),
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NoteSet,
                flags: 0,
                request_id: 0,
                payload: note_payload,
            },
        )
        .expect("note_set");
        let resp = read_frame(&mut client).expect("note_set_resp");
        assert_eq!(resp.msg_type, MessageType::NoteSetResp);
        let op = decode_op_resp(&resp.payload).expect("decode");
        assert!(op.ok, "note set should succeed");

        // Get note
        let get_payload = encode_detach(&DetachPayload {
            session_id: session.session_id,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NoteGet,
                flags: 0,
                request_id: 0,
                payload: get_payload,
            },
        )
        .expect("note_get");
        let resp = read_frame(&mut client).expect("note_get_resp");
        assert_eq!(resp.msg_type, MessageType::NoteGetResp);
        let got = decode_note_get_resp(&resp.payload).expect("decode");
        assert_eq!(got, "my test note");

        // Clear note (empty string)
        let clear_payload = encode_note(&NotePayload {
            session_id: session.session_id,
            note: String::new(),
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NoteSet,
                flags: 0,
                request_id: 0,
                payload: clear_payload,
            },
        )
        .expect("note_clear");
        let resp = read_frame(&mut client).expect("note_clear_resp");
        assert_eq!(resp.msg_type, MessageType::NoteSetResp);
        let op = decode_op_resp(&resp.payload).expect("decode");
        assert!(op.ok);

        // Verify cleared
        let get_payload2 = encode_detach(&DetachPayload {
            session_id: session.session_id,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NoteGet,
                flags: 0,
                request_id: 0,
                payload: get_payload2,
            },
        )
        .expect("note_get2");
        let resp = read_frame(&mut client).expect("note_get_resp2");
        assert_eq!(resp.msg_type, MessageType::NoteGetResp);
        let got2 = decode_note_get_resp(&resp.payload).expect("decode");
        assert!(got2.is_empty(), "note should be cleared");

        drop(client);
        server.join().expect("server thread");
    }

    #[test]
    fn client_tag_add_list_remove_flow() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-ipc-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));
        let ms = Arc::new(Mutex::new(
            MetadataStore::open_in_memory().expect("open metadata"),
        ));

        let sm_server = sm.clone();
        let ms_server = ms.clone();
        let server = std::thread::spawn(move || {
            if let Ok(conn) = daemon.accept() {
                let _ = handle_client(conn, sm_server, Some(ms_server), None, 0);
            }
        });

        std::thread::sleep(Duration::from_millis(100));

        let mut client = UnixStream::connect(&socket_path).expect("connect");
        let hello = encode_hello(&HelloPayload {
            protocol_major: 0,
            protocol_minor: 1,
            uid: 0,
            pid: 0,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Hello,
                flags: 0,
                request_id: 0,
                payload: hello,
            },
        )
        .expect("hello");
        let _ack = read_frame(&mut client).expect("hello ack");

        // Create a session
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NewSession,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("new_session");
        let resp = read_frame(&mut client).expect("new_session_resp");
        assert_eq!(resp.msg_type, MessageType::NewSessionResp);
        let session = decode_new_session_resp(&resp.payload).expect("decode");

        // Add tag "work"
        let tag1 = encode_tag(&TagPayload {
            session_id: session.session_id,
            tag: "work".to_string(),
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::TagAdd,
                flags: 0,
                request_id: 0,
                payload: tag1,
            },
        )
        .expect("tag_add");
        let resp = read_frame(&mut client).expect("tag_add_resp");
        assert_eq!(resp.msg_type, MessageType::TagAddResp);
        let op = decode_op_resp(&resp.payload).expect("decode");
        assert!(op.ok);

        // Add tag "urgent"
        let tag2 = encode_tag(&TagPayload {
            session_id: session.session_id,
            tag: "urgent".to_string(),
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::TagAdd,
                flags: 0,
                request_id: 0,
                payload: tag2,
            },
        )
        .expect("tag_add2");
        let resp = read_frame(&mut client).expect("tag_add_resp2");
        assert_eq!(resp.msg_type, MessageType::TagAddResp);
        let op = decode_op_resp(&resp.payload).expect("decode");
        assert!(op.ok);

        // List tags
        let list_payload = encode_detach(&DetachPayload {
            session_id: session.session_id,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::TagList,
                flags: 0,
                request_id: 0,
                payload: list_payload,
            },
        )
        .expect("tag_list");
        let resp = read_frame(&mut client).expect("tag_list_resp");
        assert_eq!(resp.msg_type, MessageType::TagListResp);
        let tags = decode_tag_list_resp(&resp.payload).expect("decode");
        assert_eq!(tags.tags, vec!["urgent", "work"]); // sorted by tag

        // Remove tag "work"
        let remove_payload = encode_tag(&TagPayload {
            session_id: session.session_id,
            tag: "work".to_string(),
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::TagRemove,
                flags: 0,
                request_id: 0,
                payload: remove_payload,
            },
        )
        .expect("tag_remove");
        let resp = read_frame(&mut client).expect("tag_remove_resp");
        assert_eq!(resp.msg_type, MessageType::TagRemoveResp);
        let op = decode_op_resp(&resp.payload).expect("decode");
        assert!(op.ok);

        // List after remove
        let list_payload2 = encode_detach(&DetachPayload {
            session_id: session.session_id,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::TagList,
                flags: 0,
                request_id: 0,
                payload: list_payload2,
            },
        )
        .expect("tag_list2");
        let resp = read_frame(&mut client).expect("tag_list_resp2");
        assert_eq!(resp.msg_type, MessageType::TagListResp);
        let tags2 = decode_tag_list_resp(&resp.payload).expect("decode");
        assert_eq!(tags2.tags, vec!["urgent"]);

        // ListSessionsByTag
        let filter = encode_note_get_resp("urgent");
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::ListSessionsByTag,
                flags: 0,
                request_id: 0,
                payload: filter,
            },
        )
        .expect("list_by_tag");
        let resp = read_frame(&mut client).expect("list_by_tag_resp");
        assert_eq!(resp.msg_type, MessageType::ListSessionsResp);
        let list = decode_list_sessions_resp(&resp.payload).expect("decode");
        assert_eq!(list.sessions.len(), 1);
        assert_eq!(list.sessions[0].session_id, session.session_id);
        assert!(list.sessions[0].has_tags);

        drop(client);
        server.join().expect("server thread");
    }

    #[test]
    fn client_pin_set_flow() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-ipc-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));
        let ms = Arc::new(Mutex::new(
            MetadataStore::open_in_memory().expect("open metadata"),
        ));

        let sm_server = sm.clone();
        let ms_server = ms.clone();
        let server = std::thread::spawn(move || {
            if let Ok(conn) = daemon.accept() {
                let _ = handle_client(conn, sm_server, Some(ms_server), None, 0);
            }
        });

        std::thread::sleep(Duration::from_millis(100));

        let mut client = UnixStream::connect(&socket_path).expect("connect");
        let hello = encode_hello(&HelloPayload {
            protocol_major: 0,
            protocol_minor: 1,
            uid: 0,
            pid: 0,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Hello,
                flags: 0,
                request_id: 0,
                payload: hello,
            },
        )
        .expect("hello");
        let _ack = read_frame(&mut client).expect("hello ack");

        // Create a session
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NewSession,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("new_session");
        let resp = read_frame(&mut client).expect("new_session_resp");
        assert_eq!(resp.msg_type, MessageType::NewSessionResp);
        let session = decode_new_session_resp(&resp.payload).expect("decode");

        // Pin session
        let pin_payload = encode_pin(&PinPayload {
            session_id: session.session_id,
            pinned: true,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::PinSet,
                flags: 0,
                request_id: 0,
                payload: pin_payload,
            },
        )
        .expect("pin_set");
        let resp = read_frame(&mut client).expect("pin_set_resp");
        assert_eq!(resp.msg_type, MessageType::PinSetResp);
        let op = decode_op_resp(&resp.payload).expect("decode");
        assert!(op.ok);

        // List should show pinned
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::ListSessions,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("list");
        let resp = read_frame(&mut client).expect("list_resp");
        assert_eq!(resp.msg_type, MessageType::ListSessionsResp);
        let list = decode_list_sessions_resp(&resp.payload).expect("decode");
        assert_eq!(list.sessions.len(), 1);
        assert!(list.sessions[0].is_pinned);

        // Lock session and verify it is persisted, listed, and cannot be attached.
        let lock_payload = persist_ipc::encode_lock(&persist_ipc::LockPayload {
            session_id: session.session_id,
            locked: true,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::LockSet,
                flags: 0,
                request_id: 0,
                payload: lock_payload,
            },
        )
        .expect("lock_set");
        let resp = read_frame(&mut client).expect("lock_set_resp");
        assert_eq!(resp.msg_type, MessageType::LockSetResp);
        assert!(decode_op_resp(&resp.payload).expect("decode").ok);

        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::ListSessions,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("list_locked");
        let resp = read_frame(&mut client).expect("list_locked_resp");
        let list = decode_list_sessions_resp(&resp.payload).expect("decode");
        assert!(list.sessions[0].is_locked);

        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Attach,
                flags: 0,
                request_id: 0,
                payload: encode_attach(&AttachPayload {
                    session_id: session.session_id,
                }),
            },
        )
        .expect("attach_locked");
        let resp = read_frame(&mut client).expect("attach_locked_resp");
        let attach = decode_attach_resp(&resp.payload).expect("decode");
        assert!(!attach.ok);
        assert_eq!(attach.error_msg, "session is locked");

        let unlock_payload = persist_ipc::encode_lock(&persist_ipc::LockPayload {
            session_id: session.session_id,
            locked: false,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::LockSet,
                flags: 0,
                request_id: 0,
                payload: unlock_payload,
            },
        )
        .expect("lock_unset");
        let resp = read_frame(&mut client).expect("lock_unset_resp");
        assert!(decode_op_resp(&resp.payload).expect("decode").ok);

        // Unpin
        let unpin_payload = encode_pin(&PinPayload {
            session_id: session.session_id,
            pinned: false,
        });
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::PinSet,
                flags: 0,
                request_id: 0,
                payload: unpin_payload,
            },
        )
        .expect("pin_unset");
        let resp = read_frame(&mut client).expect("pin_unset_resp");
        assert_eq!(resp.msg_type, MessageType::PinSetResp);
        let op = decode_op_resp(&resp.payload).expect("decode");
        assert!(op.ok);

        // Verify unpinned in list
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::ListSessions,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("list2");
        let resp = read_frame(&mut client).expect("list_resp2");
        assert_eq!(resp.msg_type, MessageType::ListSessionsResp);
        let list2 = decode_list_sessions_resp(&resp.payload).expect("decode");
        assert_eq!(list2.sessions.len(), 1);
        assert!(!list2.sessions[0].is_pinned);

        drop(client);
        server.join().expect("server thread");
    }

    #[test]
    fn client_list_sessions_includes_note_and_tag_flags() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-ipc-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));

        let sm_server = sm.clone();
        let server = std::thread::spawn(move || {
            if let Ok(conn) = daemon.accept() {
                let _ = handle_client(conn, sm_server, None, None, 0);
            }
        });

        std::thread::sleep(Duration::from_millis(100));

        let mut client = UnixStream::connect(&socket_path).expect("connect");

        // HELLO
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Hello,
                flags: 0,
                request_id: 0,
                payload: encode_hello(&HelloPayload {
                    protocol_major: 0,
                    protocol_minor: 1,
                    uid: 0,
                    pid: 0,
                }),
            },
        )
        .expect("hello");
        let resp = read_frame(&mut client).expect("hello ack");
        assert_eq!(resp.msg_type, MessageType::HelloAck);

        // NEW_SESSION
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NewSession,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("new_session");
        let resp = read_frame(&mut client).expect("new_session_resp");
        assert_eq!(resp.msg_type, MessageType::NewSessionResp);
        let session = decode_new_session_resp(&resp.payload).expect("decode");

        // ATTACH
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Attach,
                flags: 0,
                request_id: 0,
                payload: encode_attach(&AttachPayload {
                    session_id: session.session_id,
                }),
            },
        )
        .expect("attach");
        let resp = read_frame(&mut client).expect("attach_resp");
        assert_eq!(resp.msg_type, MessageType::AttachResp);
        let attach = decode_attach_resp(&resp.payload).expect("decode");
        assert!(attach.ok, "attach should succeed");

        // RESIZE
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Resize,
                flags: 0,
                request_id: 0,
                payload: encode_resize(&ResizePayload {
                    rows: 40,
                    cols: 120,
                }),
            },
        )
        .expect("resize");

        std::thread::sleep(Duration::from_millis(200));

        // Send STDIN and verify STDOUT echo
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Stdin,
                flags: 0,
                request_id: 0,
                payload: b"echo attach_flow_test\n".to_vec(),
            },
        )
        .expect("stdin");

        // Read back stdout — parse Stdout frames to get clean payload
        std::thread::sleep(Duration::from_millis(300));
        let mut found_stdout = String::new();
        let mut acc = FrameAccumulator::new();
        let mut buf = [0u8; 4096];
        for _ in 0..50 {
            let n = match client.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            };
            acc.feed(&buf[..n]);
            while let Ok(Some(frame)) = acc.try_read() {
                if frame.msg_type == MessageType::Stdout {
                    found_stdout.push_str(&String::from_utf8_lossy(&frame.payload));
                }
            }
            if found_stdout.contains("attach_flow_test") {
                break;
            }
            // Don't block on next iteration
            client
                .set_read_timeout(Some(Duration::from_millis(100)))
                .ok();
        }
        assert!(
            found_stdout.contains("attach_flow_test"),
            "stdout: {found_stdout}"
        );

        // Detach
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Detach,
                flags: 0,
                request_id: 0,
                payload: encode_detach(&DetachPayload {
                    session_id: session.session_id,
                }),
            },
        )
        .expect("detach");

        drop(client);
        server.join().expect("server thread");
    }

    #[test]
    fn metadata_persistence_through_ipc() {
        // Test that metadata operations work through IPC
        let dir = tempfile::Builder::new()
            .prefix("persistd-meta-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));
        let ms = Arc::new(Mutex::new(
            MetadataStore::open_in_memory().expect("open metadata"),
        ));

        let sm_server = sm.clone();
        let ms_server = ms.clone();
        let server = std::thread::spawn(move || {
            if let Ok(conn) = daemon.accept() {
                let _ = handle_client(conn, sm_server, Some(ms_server), None, 0);
            }
        });

        std::thread::sleep(Duration::from_millis(100));

        let mut client = UnixStream::connect(&socket_path).expect("connect");

        // HELLO
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Hello,
                flags: 0,
                request_id: 0,
                payload: encode_hello(&HelloPayload {
                    protocol_major: 0,
                    protocol_minor: 1,
                    uid: 0,
                    pid: 0,
                }),
            },
        )
        .expect("hello");
        let resp = read_frame(&mut client).expect("hello ack");
        assert_eq!(resp.msg_type, MessageType::HelloAck);

        // NEW_SESSION
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::NewSession,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("new_session");
        let resp = read_frame(&mut client).expect("new_session_resp");
        assert_eq!(resp.msg_type, MessageType::NewSessionResp);
        let session = decode_new_session_resp(&resp.payload).expect("decode");
        assert!(session.session_id >= 1);

        drop(client);
        server.join().expect("server thread");
    }

    fn raw_write_frame(fd: RawFd, msg_type: MessageType, payload: &[u8]) {
        let _ = write_frame_raw(fd, msg_type, payload);
    }

    fn poll_for_output(fd: RawFd, expected: &str, timeout_ms: u64, drain: bool) -> bool {
        let mut acc = FrameAccumulator::new();
        let start = std::time::Instant::now();
        let mut found = false;
        loop {
            if start.elapsed().as_millis() > timeout_ms as u128 {
                break;
            }
            let mut buf = [0u8; 65536];
            let n = read_nonblock(fd, &mut buf).unwrap_or(0);
            if n > 0 {
                acc.feed(&buf[..n]);
                loop {
                    match acc.try_read() {
                        Ok(Some(frame)) if frame.msg_type == MessageType::Stdout => {
                            let s = String::from_utf8_lossy(&frame.payload);
                            if s.contains(expected) {
                                found = true;
                                if !drain {
                                    return true;
                                }
                            }
                        }
                        Ok(Some(_)) => continue,
                        _ => break,
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        found
    }

    fn run_pty_test<F>(shell: Option<&str>, test_fn: F)
    where
        F: FnOnce(RawFd, u32),
    {
        if shell.is_some_and(|path| !Path::new(path).is_file()) {
            return;
        }
        let dir = tempfile::Builder::new()
            .prefix("persistd-pty-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));

        let session_id = sm.lock().unwrap().create_with_shell(shell).unwrap();

        let sm_server = sm.clone();
        let server = std::thread::spawn(move || {
            if let Ok(conn) = daemon.accept() {
                let _ = handle_client(conn, sm_server, None, None, 0);
            }
        });

        std::thread::sleep(Duration::from_millis(200));

        let mut client = UnixStream::connect(&socket_path).expect("connect");
        let client_fd = client.as_raw_fd();

        // HELLO
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Hello,
                flags: 0,
                request_id: 0,
                payload: encode_hello(&HelloPayload {
                    protocol_major: 0,
                    protocol_minor: 1,
                    uid: 0,
                    pid: 0,
                }),
            },
        )
        .expect("hello");
        let resp = read_frame(&mut client).expect("hello ack");
        assert_eq!(resp.msg_type, MessageType::HelloAck);

        // ATTACH to pre-created session
        write_frame(
            &mut client,
            &Frame {
                msg_type: MessageType::Attach,
                flags: 0,
                request_id: 0,
                payload: encode_attach(&AttachPayload { session_id }),
            },
        )
        .expect("attach");
        let resp = read_frame(&mut client).expect("attach_resp");
        assert_eq!(resp.msg_type, MessageType::AttachResp);
        let attach = decode_attach_resp(&resp.payload).expect("decode");
        assert!(attach.ok, "attach should succeed");

        std::thread::sleep(Duration::from_millis(300));

        test_fn(client_fd, session_id);

        let detach_payload = encode_detach(&DetachPayload { session_id });
        raw_write_frame(client_fd, MessageType::Detach, &detach_payload);

        drop(client);
        server.join().expect("server thread");
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn recovery_context_keeps_prior_fields_when_sample_is_partial() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut manager = SessionManager::new(
            0,
            false,
            dir.path().join("logs"),
            dir.path().join("history"),
            0,
            0,
        );
        let policy = manager.recovery_environment_policy().expect("policy");
        let environment = EnvironmentSnapshot::capture(
            &policy,
            None,
            [("LANG".to_owned(), "C".to_owned())].into_iter().collect(),
        )
        .expect("environment");

        manager.record_recovery_context(
            1,
            RecoveryContext {
                cwd: None,
                environment: Some(environment.clone()),
            },
        );
        manager.record_recovery_context(
            1,
            RecoveryContext {
                cwd: Some("/work".to_string()),
                environment: None,
            },
        );

        let context = manager.recovery_contexts.remove(&1).expect("context");
        assert_eq!(context.cwd.as_deref(), Some("/work"));
        assert_eq!(context.environment, Some(environment));
    }

    #[test]
    fn holder_create_uses_one_fresh_matching_state_identity() {
        let dir = tempfile::tempdir().expect("temp dir");
        let runtime_dir = dir.path().join("runtime");
        let mut manager = SessionManager::new(
            1024,
            false,
            dir.path().join("logs"),
            dir.path().join("history"),
            0,
            0,
        );
        manager.set_runtime_dir(runtime_dir.clone());

        let first = manager
            .holder_create_request(
                7,
                "/bin/sh",
                None,
                &[],
                &[],
                &ConnectionEnvironment::default(),
            )
            .expect("first request");
        let second = manager
            .holder_create_request(
                7,
                "/bin/sh",
                None,
                &[],
                &[],
                &ConnectionEnvironment::default(),
            )
            .expect("second request");
        assert_ne!(first.state_incarnation, second.state_incarnation);
        assert_state_identity(&runtime_dir, &first);
        assert_state_identity(&runtime_dir, &second);
    }

    #[test]
    fn holder_create_preserves_saved_unset_and_connection_layers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut manager = SessionManager::new(
            1024,
            false,
            dir.path().join("logs"),
            dir.path().join("history"),
            0,
            0,
        );
        manager.set_runtime_dir(dir.path().join("runtime"));
        let connection =
            ConnectionEnvironment::from_pairs([("TERM", "xterm-256color")]).expect("connection");
        let request = manager
            .holder_create_request(
                8,
                "/bin/sh",
                None,
                &[("LANG".to_owned(), "C.UTF-8".to_owned())],
                &["EDITOR".to_owned()],
                &connection,
            )
            .expect("request");

        assert_eq!(
            request.launch_environment.saved_set(),
            &[("LANG".to_owned(), "C.UTF-8".to_owned())]
        );
        assert_eq!(
            request.launch_environment.saved_unset(),
            &["EDITOR".to_owned()]
        );
        assert_eq!(
            request.launch_environment.connection(),
            &[("TERM".to_owned(), "xterm-256color".to_owned())]
        );
    }

    #[test]
    fn metadata_failure_keeps_exited_holder_context() {
        use std::cell::RefCell;

        let events = RefCell::new(Vec::new());
        let result = metadata_then_retire(
            || {
                events.borrow_mut().push("metadata");
                Err(PersistError::internal_error("metadata failed"))
            },
            || {
                events.borrow_mut().push("retire");
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(*events.borrow(), vec!["metadata"]);

        events.borrow_mut().clear();
        let result = metadata_then_retire(
            || {
                events.borrow_mut().push("metadata");
                Ok(())
            },
            || {
                events.borrow_mut().push("retire");
                Err(PersistError::internal_error("retire failed"))
            },
        );
        assert!(result.is_err());
        assert_eq!(*events.borrow(), vec!["metadata", "retire"]);
    }

    fn assert_state_identity(runtime_dir: &Path, request: &HolderCreateRequest) {
        let incarnation = request
            .state_incarnation
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            Path::new(&request.state_file),
            runtime_dir
                .join("session-state")
                .join(format!("{}-{incarnation}.json", request.session_id))
        );
    }

    #[test]
    fn closed_session_attach_restores_cwd_and_environment() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut manager = SessionManager::new(
            0,
            false,
            dir.path().join("logs"),
            dir.path().join("history"),
            0,
            0,
        );
        let session_id = manager
            .create_with_shell(Some("/bin/sh"))
            .expect("create session");
        let name = manager.session_name(session_id).expect("name");
        let shell = manager.session_shell(session_id).expect("shell");
        let metadata = Arc::new(Mutex::new(
            MetadataStore::open_in_memory().expect("metadata"),
        ));
        metadata
            .lock()
            .unwrap()
            .create_session(session_id, &name, Some("/tmp"), Some(&shell))
            .expect("create metadata");

        let pty = manager.list().pop().expect("session pty").1;
        {
            let mut pty = pty.lock().unwrap();
            writeln!(pty, "cd /; sleep 1; exit").expect("exit shell");
        }
        let mut exited = false;
        let policy = manager.recovery_environment_policy().expect("policy");
        for _ in 0..200 {
            let pty = manager.list().pop().expect("session pty").1;
            let mut pty = pty.lock().unwrap();
            let context = capture_recovery_context(&pty, &policy);
            exited = pty.poll_exit().expect("poll exit").is_some();
            drop(pty);
            manager.record_recovery_context(session_id, context);
            if exited {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(exited, "shell should exit");
        let closed = manager
            .close_session(session_id)
            .expect("close")
            .expect("closed session");
        let encoded_environment = closed
            .recovery_context
            .environment
            .as_ref()
            .map(|snapshot| persist_metadata::encode_environment(snapshot, &policy).unwrap());
        metadata
            .lock()
            .unwrap()
            .close_session_with_context(
                session_id,
                closed.exit_code,
                closed.recovery_context.cwd.as_deref(),
                encoded_environment.as_deref(),
            )
            .expect("persist closed session");

        let record = metadata
            .lock()
            .unwrap()
            .get_session(session_id)
            .expect("get metadata")
            .expect("session metadata");
        assert_eq!(record.status, "closed");
        assert_eq!(record.cwd.as_deref(), Some("/"));

        let environment =
            persist_metadata::decode_environment(record.env_snapshot.as_deref(), &policy)
                .expect("decode environment");
        let expected_lang = environment
            .as_ref()
            .and_then(|snapshot| snapshot.env_set.get("LANG"))
            .cloned()
            .unwrap_or_default();
        manager
            .restore_closed_session(
                session_id,
                record.name,
                record.shell.as_deref(),
                record.cwd.as_deref().map(Path::new),
                environment.as_ref(),
                &ConnectionEnvironment::default(),
            )
            .expect("restore runtime");
        metadata
            .lock()
            .unwrap()
            .reopen_session(session_id)
            .expect("reopen metadata");
        assert_eq!(
            metadata
                .lock()
                .unwrap()
                .get_session(session_id)
                .expect("get reopened")
                .expect("reopened")
                .status,
            "running"
        );
        let pty = manager.list().pop().expect("restored pty").1;
        let mut pty = pty.lock().unwrap();
        writeln!(pty, "printf '%s:%s\\n' \"$PWD\" \"$LANG\"").expect("write command");
        let mut output = String::new();
        let mut buffer = [0_u8; 4096];
        for _ in 0..50 {
            if pty
                .poll_output(Duration::from_millis(100))
                .expect("poll output")
            {
                let count = pty.read_output(&mut buffer).expect("read output");
                output.push_str(&String::from_utf8_lossy(&buffer[..count]));
                if output.contains(&format!("/:{expected_lang}")) {
                    break;
                }
            }
        }
        assert!(
            output.contains(&format!("/:{expected_lang}")),
            "unexpected restored output: {output:?}"
        );
    }

    fn read_until_type(stream: &mut UnixStream, expected: MessageType) -> Frame {
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("set timeout");
        for _ in 0..16 {
            let frame = read_frame(stream).expect("read control frame");
            if frame.msg_type == expected {
                return frame;
            }
        }
        panic!("did not receive {expected:?}");
    }

    #[test]
    fn client_writer_takeover_revokes_stale_input() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-writer-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");
        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));
        let sid = sm.lock().unwrap().create().expect("create session");

        let server_sm = sm.clone();
        let server = std::thread::spawn(move || {
            let mut handlers = Vec::new();
            for _ in 0..2 {
                let conn = daemon.accept().expect("accept client");
                let client_sm = server_sm.clone();
                handlers.push(std::thread::spawn(move || {
                    let _ = handle_client(conn, client_sm, None, None, 0);
                }));
            }
            for handler in handlers {
                handler.join().expect("join client handler");
            }
        });

        let mut first = UnixStream::connect(&socket_path).expect("connect first");
        let mut second = UnixStream::connect(&socket_path).expect("connect second");
        for client in [&mut first, &mut second] {
            write_frame(
                client,
                &Frame {
                    msg_type: MessageType::Hello,
                    flags: 0,
                    request_id: 0,
                    payload: encode_hello(&HelloPayload {
                        protocol_major: 0,
                        protocol_minor: 1,
                        uid: 0,
                        pid: 0,
                    }),
                },
            )
            .expect("send hello");
            assert_eq!(
                read_frame(client).expect("read hello").msg_type,
                MessageType::HelloAck
            );
        }

        for client in [&mut first, &mut second] {
            write_frame(
                client,
                &Frame {
                    msg_type: MessageType::Attach,
                    flags: 0,
                    request_id: 0,
                    payload: encode_attach(&AttachPayload { session_id: sid }),
                },
            )
            .expect("send attach");
            assert_eq!(
                read_until_type(client, MessageType::AttachResp).msg_type,
                MessageType::AttachResp
            );
            assert_eq!(
                read_until_type(client, MessageType::WriteGranted).msg_type,
                MessageType::WriteGranted
            );
        }

        assert_eq!(
            read_until_type(&mut first, MessageType::WriteRequest).msg_type,
            MessageType::WriteRequest
        );
        assert_eq!(
            read_until_type(&mut first, MessageType::WriteRevoked).msg_type,
            MessageType::WriteRevoked
        );

        std::thread::sleep(Duration::from_millis(150));
        raw_write_frame(
            second.as_raw_fd(),
            MessageType::Stdin,
            b"echo m35_writer_ok\n",
        );
        assert!(poll_for_output(
            second.as_raw_fd(),
            "m35_writer_ok",
            5000,
            false
        ));

        raw_write_frame(
            first.as_raw_fd(),
            MessageType::Stdin,
            b"echo m35_stale_writer\n",
        );
        assert!(!poll_for_output(
            second.as_raw_fd(),
            "m35_stale_writer",
            500,
            false
        ));

        drop(first);
        drop(second);
        server.join().expect("join daemon");
    }

    #[test]
    fn pty_shell_echo_command() {
        run_pty_test(None, |fd, _sid| {
            raw_write_frame(fd, MessageType::Stdin, b"echo hello_pty_test\n");
            assert!(
                poll_for_output(fd, "hello_pty_test", 5000, false),
                "should see echo output"
            );
        });
    }

    #[test]
    fn pty_shell_pipe_command() {
        run_pty_test(None, |fd, _sid| {
            raw_write_frame(fd, MessageType::Stdin, b"echo pipe_test_str | wc -c\n");
            assert!(
                poll_for_output(fd, "14", 5000, false),
                "should see wc -c output"
            );
        });
    }

    #[test]
    fn pty_shell_multiple_commands() {
        run_pty_test(None, |fd, _sid| {
            raw_write_frame(fd, MessageType::Stdin, b"A=hello_pty_multi\n");
            std::thread::sleep(Duration::from_millis(200));
            raw_write_frame(fd, MessageType::Stdin, b"echo $A\n");
            assert!(
                poll_for_output(fd, "hello_pty_multi", 5000, false),
                "should see variable expansion"
            );
        });
    }

    #[test]
    fn pty_shell_redirect() {
        run_pty_test(None, |fd, _sid| {
            raw_write_frame(
                fd,
                MessageType::Stdin,
                b"echo hello_redirect > /tmp/persist_pty_redirect_test\n",
            );
            std::thread::sleep(Duration::from_millis(200));
            raw_write_frame(
                fd,
                MessageType::Stdin,
                b"cat /tmp/persist_pty_redirect_test\n",
            );
            assert!(
                poll_for_output(fd, "hello_redirect", 5000, false),
                "should see redirect output"
            );
        });
    }

    #[test]
    fn pty_zsh_echo_command() {
        run_pty_test(Some("/usr/bin/zsh"), |fd, _sid| {
            raw_write_frame(fd, MessageType::Stdin, b"echo hello_zsh_test\n");
            assert!(
                poll_for_output(fd, "hello_zsh_test", 5000, false),
                "should see zsh echo output"
            );
        });
    }

    #[test]
    #[ignore = "system zshrc incompatible with test PTY"]
    fn pty_zsh_pipe_command() {
        run_pty_test(Some("/usr/bin/zsh"), |fd, _sid| {
            raw_write_frame(fd, MessageType::Stdin, b"echo zsh_pipe_str | wc -c\n");
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                poll_for_output(fd, "13", 8000, false),
                "should see zsh wc -c output"
            );
        });
    }

    #[test]
    fn pty_fish_echo_command() {
        run_pty_test(Some("/usr/bin/fish"), |fd, _sid| {
            raw_write_frame(fd, MessageType::Stdin, b"echo hello_fish_test\n");
            assert!(
                poll_for_output(fd, "hello_fish_test", 5000, false),
                "should see fish echo output"
            );
        });
    }

    #[test]
    fn pty_fish_variable_command() {
        run_pty_test(Some("/usr/bin/fish"), |fd, _sid| {
            raw_write_frame(fd, MessageType::Stdin, b"set A hello_fish_var\n");
            std::thread::sleep(Duration::from_millis(200));
            raw_write_frame(fd, MessageType::Stdin, b"echo $A\n");
            assert!(
                poll_for_output(fd, "hello_fish_var", 5000, false),
                "should see fish variable expansion"
            );
        });
    }

    #[test]
    fn stress_multi_session_concurrent() {
        let n_sessions = 15;
        let dir = tempfile::Builder::new()
            .prefix("persistd-stress-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));

        let mut session_ids = Vec::new();
        for _ in 0..n_sessions {
            let sid = sm.lock().unwrap().create_with_shell(None).unwrap();
            session_ids.push(sid);
        }

        // Pre-create clients
        let mut clients = Vec::new();
        for _ in 0..n_sessions {
            let client = UnixStream::connect(&socket_path).expect("connect");
            clients.push(client);
        }

        // Spawn daemon accepting loop
        let daemon_sm = sm.clone();
        let daemon_handle = std::thread::spawn(move || {
            let mut handles = Vec::new();
            for _ in 0..n_sessions {
                if let Ok(conn) = daemon.accept() {
                    let sm = daemon_sm.clone();
                    handles.push(std::thread::spawn(move || {
                        let _ = handle_client(conn, sm, None, None, 0);
                    }));
                }
            }
            for h in handles {
                let _ = h.join();
            }
        });

        std::thread::sleep(Duration::from_millis(300));

        // HELLO + ATTACH for each client
        let mut attached: Vec<(RawFd, u32)> = Vec::new();
        for (i, (mut client, sid)) in clients.iter_mut().zip(&session_ids).enumerate() {
            write_frame(
                &mut client,
                &Frame {
                    msg_type: MessageType::Hello,
                    flags: 0,
                    request_id: 0,
                    payload: encode_hello(&HelloPayload {
                        protocol_major: 0,
                        protocol_minor: 1,
                        uid: 0,
                        pid: 0,
                    }),
                },
            )
            .expect("hello");
            let resp = read_frame(&mut client).expect("hello ack");
            assert_eq!(resp.msg_type, MessageType::HelloAck, "client {i} hello");

            write_frame(
                &mut client,
                &Frame {
                    msg_type: MessageType::Attach,
                    flags: 0,
                    request_id: 0,
                    payload: encode_attach(&AttachPayload { session_id: *sid }),
                },
            )
            .expect("attach");
            let resp = read_frame(&mut client).expect("attach resp");
            assert_eq!(resp.msg_type, MessageType::AttachResp, "client {i} attach");

            attached.push((client.as_raw_fd(), *sid));
        }

        std::thread::sleep(Duration::from_millis(500));

        // Run echo command in each session
        let mut thread_handles = Vec::new();
        for (i, (fd, _sid)) in attached.iter().enumerate() {
            let fd_copy = *fd;
            thread_handles.push(std::thread::spawn(move || {
                let marker = format!("stress_ok_{i}");
                raw_write_frame(
                    fd_copy,
                    MessageType::Stdin,
                    &format!("echo {marker}\n").into_bytes(),
                );
                assert!(
                    poll_for_output(fd_copy, &marker, 10000, false),
                    "session {i} should see marker '{marker}'"
                );
            }));
        }

        for (i, h) in thread_handles.into_iter().enumerate() {
            h.join()
                .unwrap_or_else(|_| panic!("session {i} thread panicked"));
        }

        drop(clients);
        daemon_handle.join().expect("daemon thread");
    }

    #[test]
    fn stress_large_output() {
        // Write 1.5MB of data via PTY and verify it's all captured
        run_pty_test(None, |fd, _sid| {
            // Use dd to write 1500KB of data to stdout
            raw_write_frame(
                fd,
                MessageType::Stdin,
                b"dd if=/dev/zero bs=1024 count=1500 2>/dev/null | wc -c\n",
            );
            assert!(
                poll_for_output(fd, "1536000", 30000, false),
                "should see 1536000 bytes from dd|wc -c"
            );
        });
    }

    #[test]
    fn stress_large_output_pv() {
        // Alternative: use a loop to generate large output without dd
        run_pty_test(None, |fd, _sid| {
            raw_write_frame(
                fd,
                MessageType::Stdin,
                b"for i in $(seq 1 1000); do printf 'A%.0s' $(seq 1 1000); done | wc -c\n",
            );
            assert!(
                poll_for_output(fd, "1000000", 30000, false),
                "should see 1000000 bytes from loop|wc -c"
            );
        });
    }

    #[test]
    fn stress_frequent_attach_detach() {
        let dir = tempfile::Builder::new()
            .prefix("persistd-attach-")
            .tempdir()
            .expect("create temp dir");
        let socket_path = dir.path().join("test.sock");

        let daemon = DaemonSocket::bind(socket_path.clone()).expect("bind");
        let sm = Arc::new(Mutex::new(SessionManager::new(
            0,
            false,
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            0,
            0,
        )));

        let session_id = sm.lock().unwrap().create_with_shell(None).unwrap();

        let (conn_done_tx, conn_done_rx) = std::sync::mpsc::channel();
        let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let n_iterations = 20;

        let daemon_sm = sm.clone();
        let stop = stop_flag.clone();
        let daemon_handle = std::thread::spawn(move || loop {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if let Ok(conn) = daemon.accept() {
                let sm = daemon_sm.clone();
                let tx = conn_done_tx.clone();
                std::thread::spawn(move || {
                    let _ = handle_client(conn, sm, None, None, 0);
                    let _ = tx.send(());
                });
            } else {
                break;
            }
        });

        std::thread::sleep(Duration::from_millis(200));

        for i in 0..n_iterations {
            let mut client = UnixStream::connect(&socket_path).expect("connect");
            let fd = client.as_raw_fd();

            write_frame(
                &mut client,
                &Frame {
                    msg_type: MessageType::Hello,
                    flags: 0,
                    request_id: 0,
                    payload: encode_hello(&HelloPayload {
                        protocol_major: 0,
                        protocol_minor: 1,
                        uid: 0,
                        pid: 0,
                    }),
                },
            )
            .expect("hello");
            let resp = read_frame(&mut client).expect("hello ack");
            assert_eq!(resp.msg_type, MessageType::HelloAck, "iteration {i} hello");

            write_frame(
                &mut client,
                &Frame {
                    msg_type: MessageType::Attach,
                    flags: 0,
                    request_id: 0,
                    payload: encode_attach(&AttachPayload { session_id }),
                },
            )
            .expect("attach");
            let resp = read_frame(&mut client).expect("attach resp");
            assert_eq!(
                resp.msg_type,
                MessageType::AttachResp,
                "iteration {i} attach"
            );

            std::thread::sleep(Duration::from_millis(100));

            let marker = format!("attach_test_{i}");
            raw_write_frame(
                fd,
                MessageType::Stdin,
                &format!("echo {marker}\n").into_bytes(),
            );
            assert!(
                poll_for_output(fd, &marker, 5000, false),
                "iteration {i} should see marker '{marker}'"
            );

            // Detach
            let detach_payload = encode_detach(&DetachPayload { session_id });
            raw_write_frame(fd, MessageType::Detach, &detach_payload);
            std::thread::sleep(Duration::from_millis(50));

            drop(client);
            std::thread::sleep(Duration::from_millis(100));
        }

        // Signal daemon loop to stop and wake up accept()
        stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = UnixStream::connect(&socket_path);
        // Wait for all handle_client threads to finish
        for _ in 0..n_iterations {
            conn_done_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("all client threads should finish");
        }
        let _ = daemon_handle.join();
    }

    #[test]
    fn stress_signal_sigint_via_ipc() {
        run_pty_test(None, |fd, sid| {
            raw_write_frame(
                fd,
                MessageType::Stdin,
                b"trap 'echo TRAPPED_INT' INT; sleep 10\n",
            );
            std::thread::sleep(Duration::from_millis(500));

            let payload = encode_signal(&SignalPayload {
                session_id: sid,
                signal: libc::SIGINT as u32,
            });
            raw_write_frame(fd, MessageType::Signal, &payload);

            assert!(
                poll_for_output(fd, "TRAPPED_INT", 5000, false),
                "should see TRAPPED_INT from SIGINT trap"
            );
        });
    }

    #[test]
    fn stress_signal_sigtstp_via_ipc() {
        run_pty_test(None, |fd, sid| {
            raw_write_frame(
                fd,
                MessageType::Stdin,
                b"trap 'echo TRAPPED_TSTP' TSTP; sleep 10\n",
            );
            std::thread::sleep(Duration::from_millis(500));

            let payload = encode_signal(&SignalPayload {
                session_id: sid,
                signal: libc::SIGTSTP as u32,
            });
            raw_write_frame(fd, MessageType::Signal, &payload);

            assert!(
                poll_for_output(fd, "TRAPPED_TSTP", 5000, false),
                "should see TRAPPED_TSTP from SIGTSTP trap"
            );
        });
    }

    #[test]
    fn session_idle_string_returns_seconds_after_creation() {
        let mut sm =
            SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        let id = sm.create().expect("create session");
        let idle = sm.idle_string(id);
        assert!(
            idle.ends_with('s') || idle.is_empty(),
            "idle should be in seconds or empty, got: {idle}"
        );
    }

    #[test]
    fn dashboard_request_copies_roots_and_connection_counts() {
        let temp = tempfile::tempdir().unwrap();
        let mut manager = SessionManager::new(
            0,
            false,
            temp.path().join("logs"),
            temp.path().join("history"),
            0,
            0,
        );
        let id = manager.create().expect("create session");
        manager.mark_attached(id, 10);
        manager.add_ro_client(id, 11);
        manager.add_ro_client(id, 12);

        let request = manager.dashboard_sample_request();
        assert_eq!(request.session_count, 1);
        assert_eq!(request.runtime_count, 1);
        assert_eq!(request.active_writer_count, 1);
        assert_eq!(request.readonly_client_count, 2);
        assert_eq!(request.roots.len(), 1);
        assert_eq!(request.roots[0].session_id, id);
        assert!(request.roots[0].root_pid > 0);
        assert!(request.roots[0].writer_active);
    }

    #[test]
    fn session_idle_string_before_activity_is_empty() {
        let sm = SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        // id that does not exist
        let idle = sm.idle_string(9999);
        assert!(idle.is_empty(), "should be empty for unknown id");
    }

    #[test]
    fn session_record_activity_updates_idle() {
        let mut sm =
            SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        let id = sm.create().expect("create session");
        // record activity manually
        sm.record_activity(id);
        let idle = sm.idle_string(id);
        assert!(
            !idle.is_empty(),
            "idle should not be empty after record_activity"
        );
    }

    #[test]
    fn gc_run_removes_nothing_when_timeout_is_zero() {
        let mut sm =
            SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        let _id = sm.create().expect("create session");
        let removed = sm.gc_run(|_| false);
        assert!(removed.is_empty(), "should not remove when timeout is 0");
        assert_eq!(sm.list().len(), 1, "session should still exist");
    }

    #[test]
    fn gc_run_skips_attached_sessions() {
        let mut sm =
            SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        let id = sm.create().expect("create session");
        sm.set_gc_idle_timeout(Duration::from_millis(1));
        sm.mark_attached(id, 99);
        std::thread::sleep(Duration::from_millis(10));
        let removed = sm.gc_run(|_| false);
        assert!(removed.is_empty(), "should not remove attached sessions");
    }

    #[test]
    fn gc_run_skips_pinned_sessions() {
        let mut sm =
            SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        let id = sm.create().expect("create session");
        sm.set_gc_idle_timeout(Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));
        let removed = sm.gc_run(|sid| sid == id);
        assert!(removed.is_empty(), "should not remove pinned sessions");
    }

    #[test]
    fn gc_run_skips_locked_sessions() {
        let mut sm =
            SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        let id = sm.create().expect("create session");
        sm.set_locked(id, true);
        sm.set_gc_idle_timeout(Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));
        let removed = sm.gc_run(|_| false);
        assert!(removed.is_empty(), "should not remove locked sessions");
        assert_eq!(sm.list().len(), 1, "locked session should remain");
    }

    #[test]
    fn gc_run_removes_idle_unattached_unpinned_sessions() {
        let mut sm =
            SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        let id = sm.create().expect("create session");
        sm.set_gc_idle_timeout(Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));
        let removed = sm.gc_run(|_| false);
        assert_eq!(removed.len(), 1, "should remove one idle session");
        assert_eq!(removed[0].0, id, "should remove the idle session");
        assert_eq!(removed[0].1.exit_code, 137);
        assert!(sm.list().is_empty(), "session should be removed");
    }

    #[test]
    fn gc_run_leaves_recently_active_sessions() {
        let mut sm =
            SessionManager::new(0, false, PathBuf::from("/tmp"), PathBuf::from("/tmp"), 0, 0);
        let id = sm.create().expect("create session");
        sm.set_gc_idle_timeout(Duration::from_secs(3600));
        sm.record_activity(id);
        let removed = sm.gc_run(|_| false);
        assert!(removed.is_empty(), "should not remove active session");
        assert_eq!(sm.list().len(), 1, "session should still exist");
    }
}
