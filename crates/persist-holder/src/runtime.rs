use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use persist_core::shell_state::{remove_validated, EnvironmentSnapshot, ShellStateIdentity};
use persist_core::{PersistError, Result, RingBuffer};
use persist_ipc::holder::{
    CreateSessionRequest, ExitContextResponse, ExitContextResponseV2, HolderLogState,
    HolderSessionEntry, HolderSessionState, InventoryRequest, InventoryResponse, OperationResponse,
    OperationStatus, ResizeRequest, SignalRequest,
};
use persist_pty::{PtyEngine, PtySession};

use crate::log_worker::LogWorker;

mod state;

const MAX_PENDING_INPUT: usize = 1024 * 1024;

pub(crate) struct Runtime {
    sessions: BTreeMap<u32, Session>,
    pty_to_session: BTreeMap<RawFd, u32>,
    state_dir: PathBuf,
    generation: u64,
}

pub(crate) struct OutputBatch {
    pub(crate) session_id: u32,
    pub(crate) chunks: Vec<Vec<u8>>,
    pub(crate) log_dropped: u64,
}

pub(crate) struct ExitedSession {
    pub(crate) session_id: u32,
    pub(crate) exit_code: i32,
    pub(crate) cwd: Option<String>,
    pub(crate) environment: Option<EnvironmentSnapshot>,
    pub(crate) pty_fd: RawFd,
    pub(crate) closing: bool,
}

struct Session {
    pty: Option<PtySession>,
    shell_pid: u32,
    ring: RingBuffer,
    log_path: Option<PathBuf>,
    log_state: HolderLogState,
    created_at_ms: u64,
    last_active_at_ms: u64,
    state_identity: ShellStateIdentity,
    last_state_sequence: u64,
    final_cwd: Option<String>,
    final_environment: Option<EnvironmentSnapshot>,
    exit_code: Option<i32>,
    input: VecDeque<Vec<u8>>,
    input_offset: usize,
    input_bytes: usize,
    closing: bool,
}

impl Runtime {
    pub(crate) fn new(state_dir: PathBuf) -> Self {
        Self {
            sessions: BTreeMap::new(),
            pty_to_session: BTreeMap::new(),
            state_dir,
            generation: 0,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn create(
        &mut self,
        request: CreateSessionRequest,
    ) -> Result<(OperationResponse, Option<RawFd>)> {
        if self.sessions.contains_key(&request.session_id) {
            return Ok((
                response(request.session_id, OperationStatus::Conflict, "exists"),
                None,
            ));
        }
        let state_identity = match state::validated_identity(
            &self.state_dir,
            request.session_id,
            request.state_incarnation,
            &request.state_file,
        ) {
            Some(identity) => identity,
            None => {
                return Ok((
                    response(
                        request.session_id,
                        OperationStatus::Rejected,
                        "invalid state identity",
                    ),
                    None,
                ));
            }
        };
        let cwd = request.cwd.as_deref().map(Path::new);
        let pty = PtyEngine::new().open_session_with_launch_environment(
            &request.shell,
            request.history_file.as_deref(),
            cwd,
            &request.launch_environment,
            &request.arguments,
        )?;
        let fd = pty.master_fd();
        let now = now_ms();
        let log_enabled = request.log_path.is_some();
        let session = Session {
            shell_pid: pty.child_pid(),
            pty: Some(pty),
            ring: RingBuffer::new(request.ring_buffer_size as usize),
            log_path: request.log_path.map(PathBuf::from),
            log_state: if log_enabled {
                HolderLogState::Healthy
            } else {
                HolderLogState::Disabled
            },
            created_at_ms: now,
            last_active_at_ms: now,
            state_identity,
            last_state_sequence: 0,
            final_cwd: None,
            final_environment: None,
            exit_code: None,
            input: VecDeque::new(),
            input_offset: 0,
            input_bytes: 0,
            closing: false,
        };
        self.sessions.insert(request.session_id, session);
        self.pty_to_session.insert(fd, request.session_id);
        self.bump_generation();
        Ok((
            response(request.session_id, OperationStatus::Ok, ""),
            Some(fd),
        ))
    }

    pub(crate) fn inventory(&self, request: InventoryRequest) -> InventoryResponse {
        let mut matching = self
            .sessions
            .iter()
            .filter(|(id, _)| **id > request.cursor)
            .peekable();
        let mut entries = Vec::new();
        while entries.len() < request.limit as usize {
            let Some((id, session)) = matching.next() else {
                break;
            };
            entries.push(session.entry(*id));
        }
        let next_cursor = matching
            .peek()
            .and_then(|_| entries.last().map(|e| e.session_id));
        InventoryResponse {
            entries,
            next_cursor,
        }
    }

    pub(crate) fn session_for_pty(&self, fd: RawFd) -> Option<u32> {
        self.pty_to_session.get(&fd).copied()
    }

    pub(crate) fn pty_fds(&self) -> Vec<RawFd> {
        self.pty_to_session.keys().copied().collect()
    }

    pub(crate) fn pty_fd(&self, session_id: u32) -> Option<RawFd> {
        self.sessions
            .get(&session_id)?
            .pty
            .as_ref()
            .map(PtySession::master_fd)
    }

    pub(crate) fn replay(&self, session_id: u32, max_bytes: usize) -> Option<Vec<u8>> {
        Some(self.sessions.get(&session_id)?.ring.read_replay(max_bytes))
    }

    pub(crate) fn is_running(&self, session_id: u32) -> bool {
        self.sessions
            .get(&session_id)
            .is_some_and(|session| session.exit_code.is_none())
    }

    pub(crate) fn drain_output(&mut self, fd: RawFd, logs: &LogWorker) -> Result<OutputBatch> {
        let session_id = self
            .session_for_pty(fd)
            .ok_or_else(|| PersistError::invalid_argument("unknown holder PTY"))?;
        let session = self.sessions.get_mut(&session_id).unwrap();
        let mut chunks = Vec::new();
        let mut log_dropped = 0;
        let mut buffer = vec![0u8; persist_ipc::holder::MAX_HOLDER_IO_FRAME];
        while let Some(pty) = session.pty.as_mut() {
            match pty.read_output(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let chunk = buffer[..count].to_vec();
                    session.ring.write(&chunk);
                    session.last_active_at_ms = now_ms();
                    if let Some(path) = &session.log_path {
                        log_dropped += logs.enqueue(session_id, path, &chunk);
                    }
                    chunks.push(chunk);
                }
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(source) => return Err(io_error("drain holder PTY", source)),
            }
        }
        let newly_degraded = log_dropped > 0 && session.log_state != HolderLogState::Degraded;
        if newly_degraded {
            session.log_state = HolderLogState::Degraded;
        }
        if newly_degraded {
            self.bump_generation();
        }
        Ok(OutputBatch {
            session_id,
            chunks,
            log_dropped,
        })
    }

    pub(crate) fn queue_input(&mut self, session_id: u32, data: Vec<u8>) -> Result<bool> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| PersistError::invalid_argument("holder session not found"))?;
        if session.exit_code.is_some() || data.is_empty() {
            return Err(PersistError::invalid_argument(
                "holder session is not writable",
            ));
        }
        let new_size = session
            .input_bytes
            .checked_add(data.len())
            .ok_or_else(|| PersistError::invalid_argument("holder input queue overflow"))?;
        if new_size > MAX_PENDING_INPUT {
            return Err(PersistError::invalid_argument(
                "holder input queue limit exceeded",
            ));
        }
        session.input_bytes = new_size;
        session.input.push_back(data);
        session.last_active_at_ms = now_ms();
        self.flush_input(session_id)
    }

    pub(crate) fn flush_input(&mut self, session_id: u32) -> Result<bool> {
        let session = self.sessions.get_mut(&session_id).unwrap();
        while let Some(front) = session.input.front() {
            let Some(pty) = session.pty.as_mut() else {
                break;
            };
            match pty.write_input(&front[session.input_offset..]) {
                Ok(0) => break,
                Ok(count) => {
                    session.input_offset += count;
                    session.input_bytes -= count;
                    if session.input_offset == front.len() {
                        session.input.pop_front();
                        session.input_offset = 0;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(source) => return Err(io_error("write holder PTY", source)),
            }
        }
        Ok(!session.input.is_empty())
    }

    pub(crate) fn resize(&self, session_id: u32, request: ResizeRequest) -> Result<()> {
        let fd = self
            .pty_fd(session_id)
            .ok_or_else(|| PersistError::invalid_argument("holder session not running"))?;
        let size = libc::winsize {
            ws_row: request.rows,
            ws_col: request.cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &size) } != 0 {
            return Err(io_error("resize holder PTY", io::Error::last_os_error()));
        }
        Ok(())
    }

    pub(crate) fn signal(&self, session_id: u32, request: SignalRequest) -> Result<()> {
        let pty = self
            .sessions
            .get(&session_id)
            .and_then(|session| session.pty.as_ref())
            .ok_or_else(|| PersistError::invalid_argument("holder session not running"))?;
        let foreground = pty.foreground_process_group();
        let target = foreground.unwrap_or(pty.child_pid());
        let pid = foreground.map_or(target as i32, |_| -(target as i32));
        if unsafe { libc::kill(pid, request.signal as i32) } != 0 {
            return Err(io_error("signal holder PTY", io::Error::last_os_error()));
        }
        Ok(())
    }

    pub(crate) fn close(&mut self, session_id: u32) -> OperationResponse {
        if self
            .sessions
            .get(&session_id)
            .is_some_and(|session| session.exit_code.is_some())
        {
            return response(session_id, OperationStatus::Ok, "");
        }
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return response(session_id, OperationStatus::NotFound, "not found");
        };
        if let Some(pty) = &session.pty {
            let _ = pty.signal_child(libc::SIGHUP);
            session.closing = true;
        }
        response(session_id, OperationStatus::Ok, "")
    }

    pub(crate) fn kill(&self, session_id: u32) -> OperationResponse {
        let Some(session) = self.sessions.get(&session_id) else {
            return response(session_id, OperationStatus::NotFound, "not found");
        };
        let status = session
            .pty
            .as_ref()
            .and_then(|pty| pty.signal_child(libc::SIGKILL).err())
            .map_or(OperationStatus::Ok, |_| OperationStatus::Internal);
        response(session_id, status, "")
    }

    pub(crate) fn exit_context_response(&self, session_id: u32) -> ExitContextResponse {
        let Some(session) = self.sessions.get(&session_id) else {
            return exit_response(session_id, OperationStatus::NotFound, None, None);
        };
        match session.exit_code {
            Some(code) => exit_response(
                session_id,
                OperationStatus::Ok,
                Some(code),
                session.final_cwd.clone(),
            ),
            None => exit_response(session_id, OperationStatus::Conflict, None, None),
        }
    }

    pub(crate) fn exit_context_response_v2(&self, session_id: u32) -> ExitContextResponseV2 {
        let legacy = self.exit_context_response(session_id);
        ExitContextResponseV2 {
            session_id: legacy.session_id,
            status: legacy.status,
            exit_code: legacy.exit_code,
            cwd: legacy.cwd,
            environment: self
                .sessions
                .get(&session_id)
                .and_then(|session| session.final_environment.clone()),
        }
    }

    pub(crate) fn retire_exited(&mut self, session_id: u32) -> OperationResponse {
        let Some(session) = self.sessions.get(&session_id) else {
            return response(session_id, OperationStatus::NotFound, "not found");
        };
        if session.exit_code.is_none() {
            return response(session_id, OperationStatus::Conflict, "session is running");
        }
        if remove_validated(&session.state_identity).is_err() {
            return response(
                session_id,
                OperationStatus::Internal,
                "state cleanup failed",
            );
        }
        self.sessions.remove(&session_id);
        self.bump_generation();
        response(session_id, OperationStatus::Ok, "")
    }

    pub(crate) fn reap_exited(&mut self) -> Result<Vec<ExitedSession>> {
        let mut exited = Vec::new();
        for (id, session) in &mut self.sessions {
            let Some(pty) = session.pty.as_mut() else {
                continue;
            };
            if let Some(code) = pty.poll_exit()? {
                let fd = pty.master_fd();
                if let Some(state) =
                    state::capture(&session.state_identity, session.last_state_sequence)
                {
                    session.last_state_sequence = state.sequence;
                    session.final_cwd = Some(state.cwd);
                    session.final_environment = state.environment;
                }
                session.exit_code = Some(code);
                session.last_active_at_ms = now_ms();
                exited.push(ExitedSession {
                    session_id: *id,
                    exit_code: code,
                    cwd: session.final_cwd.clone(),
                    environment: session.final_environment.clone(),
                    pty_fd: fd,
                    closing: session.closing,
                });
            }
        }
        for exited_session in &exited {
            self.pty_to_session.remove(&exited_session.pty_fd);
            if let Some(session) = self.sessions.get_mut(&exited_session.session_id) {
                session.pty.take();
            }
            self.bump_generation();
        }
        Ok(exited)
    }

    pub(crate) fn mark_log_failed(&mut self, session_id: u32) {
        let newly_degraded = self
            .sessions
            .get(&session_id)
            .is_some_and(|session| session.log_state != HolderLogState::Degraded);
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.log_state = HolderLogState::Degraded;
        }
        if newly_degraded {
            self.bump_generation();
        }
    }

    fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
    }
}

impl Session {
    fn entry(&self, session_id: u32) -> HolderSessionEntry {
        HolderSessionEntry {
            session_id,
            shell_pid: self.shell_pid,
            state: if self.exit_code.is_some() {
                HolderSessionState::Exited
            } else {
                HolderSessionState::Running
            },
            exit_code: self.exit_code,
            created_at_ms: self.created_at_ms,
            last_active_at_ms: self.last_active_at_ms,
            ring_bytes: self.ring.len() as u32,
            writer_active: false,
            log_state: self.log_state,
            exit_context_available: self.exit_code.is_some(),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.pty.take();
        let _ = remove_validated(&self.state_identity);
    }
}

fn response(session_id: u32, status: OperationStatus, message: &str) -> OperationResponse {
    OperationResponse {
        session_id,
        status,
        message: message.into(),
    }
}

fn exit_response(
    session_id: u32,
    status: OperationStatus,
    exit_code: Option<i32>,
    cwd: Option<String>,
) -> ExitContextResponse {
    ExitContextResponse {
        session_id,
        status,
        exit_code,
        cwd,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn io_error(operation: &'static str, source: io::Error) -> PersistError {
    PersistError::Io { operation, source }
}
