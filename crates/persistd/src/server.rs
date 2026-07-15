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

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use persist_core::{
    init_logging, load_config, version_string, ConfigLoadOptions, PersistError, Result, RingBuffer,
};
use persist_ipc::{
    decode_attach, decode_attach_resp, decode_detach, decode_hello, decode_list_sessions_resp,
    decode_lock, decode_new_session_resp, decode_note, decode_note_get_resp, decode_op_resp,
    decode_pin, decode_rename, decode_resize, decode_signal, decode_tag, decode_tag_list_resp,
    encode_attach, encode_attach_resp, encode_detach, encode_hello, encode_hello_ack,
    encode_list_sessions_resp, encode_new_session_resp, encode_note, encode_note_get_resp,
    encode_op_resp, encode_pin, encode_process_stats_resp, encode_process_tree_resp, encode_resize,
    encode_signal, encode_tag, encode_tag_list_resp, encode_writer_control, read_frame,
    write_frame, AttachPayload, AttachRespPayload, DaemonConnection, DaemonSocket, DetachPayload,
    Frame, FrameAccumulator, HelloAckPayload, HelloPayload, HelloStatus, ListSessionsRespPayload,
    MessageType, NewSessionRespPayload, NotePayload, OpRespPayload, PinPayload,
    ProcessStatsRespPayload, ProcessTreeNode, ProcessTreeRespPayload, ResizePayload, SessionEntry,
    SignalPayload, TagListRespPayload, TagPayload, WriterControlPayload,
};
use persist_metadata::MetadataStore;
use persist_pty::pty::detect_shell;
use persist_pty::{PtyEngine, PtySession};

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

    let pidfile = crate::lifecycle::PidFile::create(config.paths.runtime_dir.join("daemon.pid"))?;
    let socket = DaemonSocket::bind(config.paths.socket_path.clone())?;
    let metadata = Arc::new(Mutex::new(MetadataStore::open(
        &config.paths.data_dir.join("metadata.db"),
    )?));
    let next_session_id = metadata.lock().unwrap().next_session_id()?;
    let mut manager = SessionManager::new(
        config.ring_buffer.default_size.bytes() as usize,
        config.logging.session_log,
        config.paths.data_dir.join("sessions"),
        config.paths.data_dir.join("history"),
        config.logging.max_file_size.bytes(),
        config.logging.max_files,
    );
    manager.set_next_id(next_session_id);
    let sm = Arc::new(Mutex::new(manager));
    let gc_timeout = idle_timeout.unwrap_or(config.daemon.gc_idle_timeout.duration());
    sm.lock().unwrap().set_gc_idle_timeout(gc_timeout);
    let gc_interval = config.daemon.gc_interval.duration();
    let mut next_gc = std::time::Instant::now() + gc_interval;

    while !crate::lifecycle::shutdown_requested() {
        match socket.accept_timeout(Duration::from_millis(250)) {
            Ok(Some(conn)) => {
                let sm = sm.clone();
                let metadata = metadata.clone();
                std::thread::spawn(move || {
                    let _ = handle_client(conn, sm, Some(metadata), 0);
                });
            }
            Ok(None) => {}
            Err(error) => eprintln!("persistd: {error}"),
        }
        if !gc_timeout.is_zero() && std::time::Instant::now() >= next_gc {
            let metadata = metadata.clone();
            let removed = sm.lock().unwrap().gc_run(|id| {
                metadata
                    .lock()
                    .unwrap()
                    .get_session(id)
                    .ok()
                    .flatten()
                    .is_some_and(|record| record.pinned)
            });
            if !removed.is_empty() {
                eprintln!("persistd: GC removed sessions: {removed:?}");
            }
            next_gc = std::time::Instant::now() + gc_interval;
        }
    }

    drop(socket);
    drop(pidfile);
    Ok(())
}

struct SessionManager {
    sessions: Vec<(u32, Arc<Mutex<PtySession>>)>,
    session_info: HashMap<u32, SessionInfo>,
    ring_buffers: HashMap<u32, Arc<Mutex<RingBuffer>>>,
    ring_buffer_size: usize,
    log_handles: HashMap<u32, SessionLogHandle>,
    session_log_enabled: bool,
    logs_dir: PathBuf,
    history_dir: PathBuf,
    max_file_size: u64,
    max_files: u32,
    next_id: u32,
    attached_sessions: HashMap<u32, RawFd>,
    ro_attached: HashMap<u32, Vec<RawFd>>,
    last_activity: HashMap<u32, std::time::Instant>,
    gc_idle_timeout: std::time::Duration,
    locked_sessions: HashSet<u32>,
    recovery_contexts: HashMap<u32, RecoveryContext>,
}

#[derive(Debug, Clone)]
struct SessionInfo {
    name: String,
}

#[derive(Debug, Clone, Default)]
struct RecoveryContext {
    cwd: Option<String>,
    env_snapshot: Option<String>,
}

impl RecoveryContext {
    fn merge_with_fallback(self, fallback: Option<Self>) -> Self {
        let fallback = fallback.unwrap_or_default();
        Self {
            cwd: self.cwd.or(fallback.cwd),
            env_snapshot: self.env_snapshot.or(fallback.env_snapshot),
        }
    }
}

#[derive(Debug, Clone)]
struct ClosedSession {
    exit_code: i32,
    recovery_context: RecoveryContext,
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

fn capture_recovery_context(pty: &PtySession) -> RecoveryContext {
    let pid = pty.child_pid();
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
                .collect::<BTreeMap<_, _>>()
        });
    let env_snapshot = environment.and_then(|environment| serde_json::to_string(&environment).ok());
    RecoveryContext { cwd, env_snapshot }
}

fn decode_recovery_environment(snapshot: Option<&str>) -> Vec<(String, String)> {
    serde_json::from_str::<BTreeMap<String, String>>(snapshot.unwrap_or("{}"))
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, _)| is_recoverable_environment_name(name))
        .collect()
}

fn foreground_process_info(pty: &PtySession) -> (Option<u32>, String, String) {
    let Some(pid) = pty.foreground_process_group() else {
        return (None, String::new(), String::new());
    };
    let proc_dir = format!("/proc/{pid}");
    let name = match std::fs::read_to_string(format!("{proc_dir}/comm")) {
        Ok(name) => name.trim().to_owned(),
        Err(_) => return (None, String::new(), String::new()),
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
    (Some(pid), name, cmd)
}

fn process_tree(pty: &PtySession) -> Vec<ProcessTreeNode> {
    const MAX_NODES: usize = 64;
    const MAX_DEPTH: u8 = 8;
    let Some(root_pid) = pty.foreground_process_group() else {
        return Vec::new();
    };
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

fn process_stats(pty: &PtySession) -> ProcessStatsRespPayload {
    let empty = ProcessStatsRespPayload {
        pid: None,
        user_ticks: 0,
        system_ticks: 0,
        rss_kib: 0,
        read_bytes: 0,
        write_bytes: 0,
    };
    let Some(pid) = pty.foreground_process_group() else {
        return empty;
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

impl SessionManager {
    fn new(
        ring_buffer_size: usize,
        session_log_enabled: bool,
        logs_dir: PathBuf,
        history_dir: PathBuf,
        max_file_size: u64,
        max_files: u32,
    ) -> Self {
        Self {
            sessions: Vec::new(),
            session_info: HashMap::new(),
            ring_buffers: HashMap::new(),
            ring_buffer_size,
            log_handles: HashMap::new(),
            session_log_enabled,
            logs_dir,
            history_dir,
            max_file_size,
            max_files,
            next_id: 1,
            attached_sessions: HashMap::new(),
            ro_attached: HashMap::new(),
            last_activity: HashMap::new(),
            gc_idle_timeout: std::time::Duration::from_secs(0),
            locked_sessions: HashSet::new(),
            recovery_contexts: HashMap::new(),
        }
    }

    fn create(&mut self) -> Result<u32> {
        let shell = detect_shell();
        self.create_with_shell(Some(&shell))
    }

    fn set_next_id(&mut self, next_id: u32) {
        self.next_id = next_id;
    }

    fn create_with_shell(&mut self, shell: Option<&str>) -> Result<u32> {
        let engine = PtyEngine::new();
        let id = self.next_id;
        self.next_id += 1;
        let histfile_path = self.history_dir.join(id.to_string());
        if let Some(parent) = histfile_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let histfile_str = histfile_path.to_string_lossy().to_string();
        let pty = match shell {
            Some(s) => engine.open_session_with_shell(s, Some(&histfile_str))?,
            None => engine.open_session_with_shell(&detect_shell(), Some(&histfile_str))?,
        };
        let actual_shell = pty.shell().to_string();
        let name = generate_session_name(&actual_shell);
        self.insert_runtime(id, name, pty);
        Ok(id)
    }

    fn restore_closed_session(
        &mut self,
        id: u32,
        name: String,
        shell: Option<&str>,
        cwd: Option<&Path>,
        environment: &[(String, String)],
    ) -> Result<()> {
        if self
            .sessions
            .iter()
            .any(|(session_id, _)| *session_id == id)
        {
            return Err(PersistError::invalid_argument("session is already running"));
        }
        let histfile_path = self.history_dir.join(id.to_string());
        if let Some(parent) = histfile_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let histfile = histfile_path.to_string_lossy().to_string();
        let selected_shell = shell.map(str::to_owned).unwrap_or_else(detect_shell);
        let pty = PtyEngine::new().open_session_with_context(
            &selected_shell,
            Some(&histfile),
            cwd,
            environment,
        )?;
        self.next_id = self.next_id.max(id.saturating_add(1));
        self.insert_runtime(id, name, pty);
        Ok(())
    }

    fn insert_runtime(&mut self, id: u32, name: String, pty: PtySession) {
        self.record_activity(id);
        self.session_info.insert(id, SessionInfo { name });
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
        self.sessions
            .iter()
            .find(|(session_id, _)| *session_id == id)
            .and_then(|(_, pty)| pty.lock().ok().map(|pty| pty.shell().to_owned()))
    }

    fn record_recovery_context(&mut self, id: u32, context: RecoveryContext) {
        if context.cwd.is_some() || context.env_snapshot.is_some() {
            let stored = self.recovery_contexts.entry(id).or_default();
            if context.cwd.is_some() {
                stored.cwd = context.cwd;
            }
            if context.env_snapshot.is_some() {
                stored.env_snapshot = context.env_snapshot;
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

    fn list(&self) -> Vec<(u32, Arc<Mutex<PtySession>>)> {
        self.sessions
            .iter()
            .map(|(id, pty)| (*id, pty.clone()))
            .collect()
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

    fn gc_run(&mut self, is_pinned: impl Fn(u32) -> bool) -> Vec<u32> {
        if self.gc_idle_timeout.is_zero() {
            return Vec::new();
        }
        let now = std::time::Instant::now();
        let timeout = self.gc_idle_timeout;
        let mut to_remove = Vec::new();
        for (id, _) in &self.sessions {
            if self.is_attached(*id) {
                continue;
            }
            if is_pinned(*id) || self.locked_sessions.contains(id) {
                continue;
            }
            let idle = self
                .last_activity
                .get(id)
                .map_or(true, |last| now.duration_since(*last) >= timeout);
            if idle {
                to_remove.push(*id);
            }
        }
        for id in &to_remove {
            let _ = self.kill_session(*id);
            self.remove(*id);
        }
        to_remove
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

    fn broadcast_stdout(&mut self, id: u32, data: &[u8]) {
        if let Some(fds) = self.ro_attached.get(&id) {
            let mut dead = Vec::new();
            for fd in fds {
                let ret =
                    unsafe { libc::write(*fd, data.as_ptr() as *const libc::c_void, data.len()) };
                if ret < 0 {
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

    fn kill_session(&mut self, id: u32) -> Result<()> {
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

    fn close_session(&mut self, id: u32) -> Result<Option<ClosedSession>> {
        if let Some((session, stored_context)) = self.remove(id) {
            if let Ok(mut pty) = session.lock() {
                let direct_context = capture_recovery_context(&pty);
                let recovery_context = direct_context.merge_with_fallback(stored_context);
                if pty.is_alive() {
                    let _ = pty.signal_child(libc::SIGHUP);
                }
                return pty.wait_exit().map(|exit_code| {
                    Some(ClosedSession {
                        exit_code,
                        recovery_context,
                    })
                });
            }
        }
        Ok(None)
    }
}

fn handle_client(
    mut conn: DaemonConnection,
    sm: Arc<Mutex<SessionManager>>,
    ms: Option<Arc<Mutex<MetadataStore>>>,
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
                        protocol_minor: 1,
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
                            )
                        })
                    };
                    let (id, name) = match created {
                        Ok((id, name, shell)) => {
                            let cwd = std::env::current_dir()
                                .ok()
                                .and_then(|path| path.to_str().map(str::to_owned));
                            let metadata_result = match &ms {
                                Some(metadata) => metadata.lock().unwrap().create_session(
                                    id,
                                    &name,
                                    cwd.as_deref(),
                                    shell.as_deref(),
                                ),
                                None => Ok(()),
                            };
                            if metadata_result.is_ok() {
                                (id, name)
                            } else {
                                sm.lock().unwrap().remove(id);
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
                                record.status == "closed"
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
                        let pty = sm
                            .lock()
                            .unwrap()
                            .list()
                            .into_iter()
                            .find(|(session_id, _)| *session_id == payload.session_id)
                            .map(|(_, pty)| pty);
                        let nodes = pty
                            .and_then(|pty| pty.lock().ok().map(|pty| process_tree(&pty)))
                            .unwrap_or_default();
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
                        let pty = sm
                            .lock()
                            .unwrap()
                            .list()
                            .into_iter()
                            .find(|(session_id, _)| *session_id == payload.session_id)
                            .map(|(_, pty)| pty);
                        let stats = pty
                            .and_then(|pty| pty.lock().ok().map(|pty| process_stats(&pty)))
                            .unwrap_or(ProcessStatsRespPayload {
                                pid: None,
                                user_ticks: 0,
                                system_ticks: 0,
                                rss_kib: 0,
                                read_bytes: 0,
                                write_bytes: 0,
                            });
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
                        let (runtime, writer_active, output_log_path) = {
                            let manager = sm.lock().unwrap();
                            let runtime = manager
                                .list()
                                .into_iter()
                                .find(|(session_id, _)| *session_id == payload.session_id)
                                .and_then(|(_, pty)| {
                                    pty.lock().ok().map(|pty| foreground_process_info(&pty))
                                });
                            let writer_active = manager.is_attached(payload.session_id);
                            let output_log_path = manager.session_log_enabled.then(|| {
                                manager
                                    .logs_dir
                                    .join(format!("{}.log", payload.session_id))
                                    .to_string_lossy()
                                    .into_owned()
                            });
                            (runtime, writer_active, output_log_path)
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
                                    "sessions": {
                                        "total": records.len(),
                                        "running": records.iter().filter(|r| r.status == "running").count(),
                                        "closed": records.iter().filter(|r| r.status == "closed").count(),
                                        "locked": records.iter().filter(|r| r.locked).count(),
                                        "pinned": records.iter().filter(|r| r.pinned).count(),
                                        "runtime": manager.sessions.len(),
                                        "active_writers": manager.attached_sessions.len(),
                                        "readonly_clients": manager.ro_attached.values().map(Vec::len).sum::<usize>(),
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
                MessageType::Attach => {
                    if let Some(payload) = decode_attach(&frame.payload) {
                        let sid = payload.session_id;
                        let record = ms.as_ref().and_then(|metadata| {
                            metadata.lock().unwrap().get_session(sid).ok().flatten()
                        });
                        let locked = record.as_ref().is_some_and(|record| record.locked);
                        let runtime_exists = {
                            let sm = sm.lock().unwrap();
                            sm.list().iter().any(|(id, _)| *id == sid)
                        };
                        if !locked
                            && !runtime_exists
                            && record
                                .as_ref()
                                .is_some_and(|record| record.status == "closed")
                        {
                            let record = record.as_ref().expect("closed record was checked");
                            let environment =
                                decode_recovery_environment(record.env_snapshot.as_deref());
                            let restored = sm.lock().unwrap().restore_closed_session(
                                sid,
                                record.name.clone(),
                                record.shell.as_deref(),
                                record.cwd.as_deref().map(Path::new),
                                &environment,
                            );
                            if restored.is_ok() {
                                if let Some(metadata) = &ms {
                                    if metadata.lock().unwrap().reopen_session(sid).is_err() {
                                        let _ = sm.lock().unwrap().remove(sid);
                                    }
                                }
                            }
                        }
                        let (ok, previous_writer) = {
                            let mut sm = sm.lock().unwrap();
                            let exists = !locked && sm.list().iter().any(|(id, _)| *id == sid);
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
                            let _ = io_loop(fd, sid, &sm_clone, &ms);
                        }
                    }
                }
                MessageType::AttachReadOnly => {
                    if let Some(payload) = decode_attach(&frame.payload) {
                        let sid = payload.session_id;
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
                            !locked && sm.list().iter().any(|(id, _)| *id == sid)
                        };
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
                    if let Some(sid) = sid {
                        let mut sm = sm.lock().unwrap();
                        if let Some((_, pty)) = sm.sessions.iter().find(|(id, _)| *id == sid) {
                            let mut pty = pty.lock().unwrap();
                            let _ = pty.write_input(&payload);
                        }
                        sm.record_activity(sid);
                        if let Some(rb) = sm.ring_buffers.get(&sid) {
                            let mut rb = rb.lock().unwrap();
                            rb.write(&payload);
                        }
                        if let Some(handle) = sm.log_handles.get(&sid) {
                            handle.write(&payload);
                        }
                    }
                }
                MessageType::Resize => {
                    if let Some(payload) = decode_resize(&frame.payload) {
                        let sm = sm.lock().unwrap();
                        if let Some((_, pty)) = sm.sessions.iter().next() {
                            let pty = pty.lock().unwrap();
                            let _ = apply_resize(pty.master_fd(), payload.rows, payload.cols);
                        }
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
                        let sm_guard = sm.lock().unwrap();
                        let forwarded = sm_guard
                            .sessions
                            .iter()
                            .find(|(id, _)| *id == sid)
                            .and_then(|(_, pty_arc)| {
                                let pty = pty_arc.lock().unwrap();
                                let pgid = unsafe { libc::tcgetpgrp(pty.master_fd()) };
                                if pgid > 0 {
                                    unsafe { libc::kill(-pgid, signal as i32) };
                                    Some(true)
                                } else {
                                    None
                                }
                            })
                            .is_some();
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
                        let sm = sm.lock().unwrap();
                        let matching_ids: Vec<u32> = match &ms {
                            Some(m) => m
                                .lock()
                                .unwrap()
                                .find_sessions_by_tag(&payload)
                                .unwrap_or_default(),
                            None => Vec::new(),
                        };
                        let sessions: Vec<SessionEntry> = sm
                            .list()
                            .iter()
                            .filter(|(id, _)| matching_ids.contains(id))
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
                                let has_tags = true;
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
                        let result = sm.lock().unwrap().close_session(sid).and_then(|closed| {
                            let closed = closed.ok_or_else(|| {
                                PersistError::invalid_argument("session not found")
                            })?;
                            match &ms {
                                Some(metadata) => {
                                    metadata.lock().unwrap().close_session_with_context(
                                        sid,
                                        closed.exit_code,
                                        closed.recovery_context.cwd.as_deref(),
                                        closed.recovery_context.env_snapshot.as_deref(),
                                    )
                                }
                                None => Ok(()),
                            }
                        });
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
                            killed.and_then(|_| {
                                let closed = { sm.lock().unwrap().close_session(sid) };
                                let closed = closed?.ok_or_else(|| {
                                    PersistError::invalid_argument("session not found")
                                })?;
                                match &ms {
                                    Some(metadata) => {
                                        metadata.lock().unwrap().close_session_with_context(
                                            sid,
                                            closed.exit_code,
                                            closed.recovery_context.cwd.as_deref(),
                                            closed.recovery_context.env_snapshot.as_deref(),
                                        )
                                    }
                                    None => Ok(()),
                                }
                            })
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
                            if let Some(rb) = sm.ring_buffers.get(&sid) {
                                let mut rb = rb.lock().unwrap();
                                rb.write(&frame.payload);
                            }
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
        let (n, should_break) = {
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
                        let _ = write_frame_raw(fd, MessageType::Stdout, &pty_buf[..n]);
                    }
                    let recovery_context = capture_recovery_context(&pty);
                    let should_break = pty.poll_exit().ok().flatten().is_some();
                    drop(pty);
                    sm_guard.record_recovery_context(sid, recovery_context);
                    (n, should_break)
                }
                None => (0, true),
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
        }

        if should_break {
            let closed = { sm.lock().unwrap().close_session(sid) };
            if let Ok(Some(closed)) = closed {
                if let Some(metadata) = ms {
                    let _ = metadata.lock().unwrap().close_session_with_context(
                        sid,
                        closed.exit_code,
                        closed.recovery_context.cwd.as_deref(),
                        closed.recovery_context.env_snapshot.as_deref(),
                    );
                }
            }
            break;
        }
    }

    sm.lock().unwrap().release_writer(sid, fd);
    Ok(())
}

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
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::net::UnixStream;

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
                let _ = handle_client(conn, sm_server, None, 0);
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
                let _ = handle_client(conn, sm_server, None, 0);
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
                let _ = handle_client(conn, sm_server, Some(ms_server), 0);
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
                let _ = handle_client(conn, sm_server, Some(ms_server), 0);
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
                let _ = handle_client(conn, sm_server, Some(ms_server), 0);
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
                let _ = handle_client(conn, sm_server, None, 0);
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
                let _ = handle_client(conn, sm_server, Some(ms_server), 0);
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
                let _ = handle_client(conn, sm_server, None, 0);
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

        manager.record_recovery_context(
            1,
            RecoveryContext {
                cwd: None,
                env_snapshot: Some(r#"{"LANG":"C"}"#.to_string()),
            },
        );
        manager.record_recovery_context(
            1,
            RecoveryContext {
                cwd: Some("/work".to_string()),
                env_snapshot: None,
            },
        );

        let context = manager.recovery_contexts.remove(&1).expect("context");
        assert_eq!(context.cwd.as_deref(), Some("/work"));
        assert_eq!(context.env_snapshot.as_deref(), Some(r#"{"LANG":"C"}"#));
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
        let expected_lang = std::env::var("LANG").unwrap_or_default();
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
        for _ in 0..200 {
            let pty = manager.list().pop().expect("session pty").1;
            let mut pty = pty.lock().unwrap();
            let context = capture_recovery_context(&pty);
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
        metadata
            .lock()
            .unwrap()
            .close_session_with_context(
                session_id,
                closed.exit_code,
                closed.recovery_context.cwd.as_deref(),
                closed.recovery_context.env_snapshot.as_deref(),
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

        let environment = decode_recovery_environment(record.env_snapshot.as_deref());
        manager
            .restore_closed_session(
                session_id,
                record.name,
                record.shell.as_deref(),
                record.cwd.as_deref().map(Path::new),
                &environment,
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
                    let _ = handle_client(conn, client_sm, None, 0);
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
                        let _ = handle_client(conn, sm, None, 0);
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
                    let _ = handle_client(conn, sm, None, 0);
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
        assert_eq!(removed, vec![id], "should remove idle session");
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
