use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use persist_core::{PersistError, Result};
use persist_ipc::holder::*;

use super::ExitContext;

const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PENDING_EVENTS: usize = 1024;
const MAX_INVENTORY_RETRIES: usize = 8;

#[derive(Clone)]
pub(super) struct ControlInventorySnapshot {
    pub(super) instance_id: [u8; 16],
    pub(super) generation: u64,
    pub(super) entries: Vec<HolderSessionEntry>,
}

pub(crate) struct HolderControlClient {
    stream: Mutex<UnixStream>,
    path: PathBuf,
    instance_id: [u8; 16],
    holder_pid: u32,
    nonce: Mutex<[u8; 16]>,
    next_request_id: AtomicU32,
    generation: AtomicU64,
    protocol_minor: AtomicU16,
    capabilities: AtomicU64,
    events: Mutex<VecDeque<HolderFrame>>,
}

impl HolderControlClient {
    pub(crate) fn connect(path: &Path) -> Result<Self> {
        let client = Self::connect_baseline(path)?;
        if client.negotiate_capabilities().is_err() {
            client.reconnect_baseline()?;
        }
        Ok(client)
    }

    fn connect_baseline(path: &Path) -> Result<Self> {
        let mut stream = UnixStream::connect(path)
            .map_err(|source| io_error("connect holder control socket", source))?;
        configure_stream(&stream)?;
        let nonce = random_nonce()?;
        let request_id = 1;
        write_frame(
            &mut stream,
            &HolderFrame {
                message_type: HolderMessageType::ControlHello,
                flags: 0,
                request_id,
                generation: 0,
                payload: encode_control_hello(&ControlHello {
                    uid: unsafe { libc::getuid() },
                    daemon_pid: std::process::id(),
                    nonce,
                }),
            },
        )?;
        let frame = read_frame(&mut stream)?;
        validate_response(&frame, request_id, HolderMessageType::ControlHelloAck)?;
        let ack = decode_control_hello_ack(&frame.payload)
            .map_err(|error| protocol_error("ControlHelloAck", error))?;
        if ack.nonce != nonce || ack.status != HelloStatus::Accepted {
            return Err(PersistError::invalid_argument(format!(
                "holder rejected control claim: {:?}",
                ack.status
            )));
        }
        Ok(Self {
            stream: Mutex::new(stream),
            path: path.to_path_buf(),
            instance_id: ack.instance_id,
            holder_pid: ack.holder_pid,
            nonce: Mutex::new(nonce),
            next_request_id: AtomicU32::new(2),
            generation: AtomicU64::new(frame.generation),
            protocol_minor: AtomicU16::new(HOLDER_PROTOCOL_BASELINE_MINOR),
            capabilities: AtomicU64::new(0),
            events: Mutex::new(VecDeque::new()),
        })
    }

    #[cfg(test)]
    pub(crate) fn connect_legacy_for_test(path: &Path) -> Result<Self> {
        Self::connect_baseline(path)
    }

    fn negotiate_capabilities(&self) -> Result<()> {
        let response = self.request_with_minor(
            HolderMessageType::Capability,
            HolderMessageType::CapabilityResp,
            encode_capability_request(&CapabilityRequest {
                instance_id: self.instance_id,
                nonce: self.nonce(),
                max_minor: HOLDER_PROTOCOL_MINOR,
            }),
            HOLDER_PROTOCOL_MINOR,
        )?;
        let response = decode_capability_response(&response.payload)
            .map_err(|error| protocol_error("CapabilityResp", error))?;
        if response.instance_id != self.instance_id || response.nonce != self.nonce() {
            return Err(PersistError::invalid_argument(
                "holder capability identity mismatch",
            ));
        }
        self.protocol_minor
            .store(response.selected_minor, Ordering::Release);
        self.capabilities
            .store(response.capabilities, Ordering::Release);
        Ok(())
    }

    pub(crate) fn instance_id(&self) -> [u8; 16] {
        self.instance_id
    }

    pub(crate) fn holder_pid(&self) -> u32 {
        self.holder_pid
    }

    pub(crate) fn nonce(&self) -> [u8; 16] {
        *self.nonce.lock().unwrap()
    }

    pub(crate) fn protocol_minor(&self) -> u16 {
        self.protocol_minor.load(Ordering::Acquire)
    }

    pub(crate) fn has_capability(&self, capability: u64) -> bool {
        self.capabilities.load(Ordering::Acquire) & capability != 0
    }

    pub(crate) fn reconnect(&self) -> Result<()> {
        let replacement = if self.protocol_minor() == HOLDER_PROTOCOL_BASELINE_MINOR {
            Self::connect_baseline(&self.path)?
        } else {
            Self::connect(&self.path)?
        };
        if replacement.instance_id != self.instance_id {
            return Err(PersistError::invalid_argument(
                "holder instance changed during reconnect",
            ));
        }
        let generation = replacement.generation.load(Ordering::Acquire);
        let nonce = *replacement.nonce.lock().unwrap();
        let protocol_minor = replacement.protocol_minor();
        let capabilities = replacement.capabilities.load(Ordering::Acquire);
        let stream = replacement.stream.into_inner().unwrap();
        *self.stream.lock().unwrap() = stream;
        *self.nonce.lock().unwrap() = nonce;
        self.generation.store(generation, Ordering::Release);
        self.protocol_minor.store(protocol_minor, Ordering::Release);
        self.capabilities.store(capabilities, Ordering::Release);
        self.events.lock().unwrap().clear();
        Ok(())
    }

    fn reconnect_baseline(&self) -> Result<()> {
        let replacement = Self::connect_baseline(&self.path)?;
        if replacement.instance_id != self.instance_id {
            return Err(PersistError::invalid_argument(
                "holder instance changed after capability probe",
            ));
        }
        *self.stream.lock().unwrap() = replacement.stream.into_inner().unwrap();
        *self.nonce.lock().unwrap() = *replacement.nonce.lock().unwrap();
        self.generation.store(
            replacement.generation.load(Ordering::Acquire),
            Ordering::Release,
        );
        self.protocol_minor
            .store(HOLDER_PROTOCOL_BASELINE_MINOR, Ordering::Release);
        self.capabilities.store(0, Ordering::Release);
        self.events.lock().unwrap().clear();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inventory(&self) -> Result<Vec<HolderSessionEntry>> {
        Ok(self.inventory_snapshot()?.entries)
    }

    pub(super) fn inventory_snapshot(&self) -> Result<ControlInventorySnapshot> {
        for _ in 0..MAX_INVENTORY_RETRIES {
            if let Some(snapshot) = self.inventory_snapshot_once()? {
                return Ok(snapshot);
            }
        }
        Err(PersistError::internal_error(
            "holder inventory generation did not stabilize",
        ))
    }

    fn inventory_snapshot_once(&self) -> Result<Option<ControlInventorySnapshot>> {
        let mut cursor = 0;
        let mut entries = Vec::new();
        let mut snapshot_generation = None;
        loop {
            let frame = self.request(
                HolderMessageType::Inventory,
                HolderMessageType::InventoryResp,
                encode_inventory_request(&InventoryRequest {
                    cursor,
                    limit: MAX_INVENTORY_ENTRIES,
                }),
            )?;
            let response = decode_inventory_response(&frame.payload)
                .map_err(|error| protocol_error("InventoryResp", error))?;
            if snapshot_generation.is_some_and(|generation| generation != frame.generation) {
                return Ok(None);
            }
            snapshot_generation = Some(frame.generation);
            entries.extend(response.entries);
            let Some(next) = response.next_cursor else {
                let generation = snapshot_generation.unwrap_or_default();
                self.discard_events_through(generation);
                return Ok(Some(ControlInventorySnapshot {
                    instance_id: self.instance_id,
                    generation,
                    entries,
                }));
            };
            if next <= cursor {
                return Err(PersistError::invalid_argument(
                    "holder inventory cursor did not advance",
                ));
            }
            cursor = next;
        }
    }

    pub(crate) fn create(&self, request: CreateSessionRequest) -> Result<()> {
        let payload = if self.has_capability(HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT) {
            encode_create_request_v2(&request)
        } else {
            encode_create_request(&request)
        }
        .map_err(|error| protocol_error("Create", error))?;
        let frame = self.request(
            HolderMessageType::Create,
            HolderMessageType::CreateResp,
            payload,
        )?;
        require_operation_ok(&frame.payload, "Create")
    }

    pub(crate) fn close(&self, session_id: u32) -> Result<()> {
        self.operation(
            HolderMessageType::Close,
            HolderMessageType::CloseResp,
            session_id,
        )
    }

    pub(crate) fn exit_context(&self, session_id: u32) -> Result<ExitContext> {
        let frame = self.request(
            HolderMessageType::GetExitContext,
            HolderMessageType::GetExitContextResp,
            encode_operation_request(&OperationRequest { session_id }),
        )?;
        let context = if self.has_capability(HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT) {
            let response = decode_exit_context_response_v2(&frame.payload)
                .map_err(|error| protocol_error("GetExitContextResp v2", error))?;
            ExitContext {
                session_id: response.session_id,
                exit_code: response
                    .exit_code
                    .ok_or_else(|| PersistError::invalid_argument("missing holder exit code"))?,
                cwd: response.cwd,
                environment: response.environment,
            }
        } else {
            let response = decode_exit_context_response(&frame.payload)
                .map_err(|error| protocol_error("GetExitContextResp", error))?;
            ExitContext {
                session_id: response.session_id,
                exit_code: response
                    .exit_code
                    .ok_or_else(|| PersistError::invalid_argument("missing holder exit code"))?,
                cwd: response.cwd,
                environment: None,
            }
        };
        if context.session_id != session_id {
            return Err(PersistError::invalid_argument(format!(
                "holder GetExitContext returned Session {}",
                context.session_id
            )));
        }
        Ok(context)
    }

    pub(crate) fn retire_exited(&self, session_id: u32) -> Result<()> {
        self.operation(
            HolderMessageType::RetireExited,
            HolderMessageType::RetireExitedResp,
            session_id,
        )
    }

    pub(crate) fn kill(&self, session_id: u32) -> Result<()> {
        self.operation(
            HolderMessageType::Kill,
            HolderMessageType::KillResp,
            session_id,
        )
    }

    pub(crate) fn shutdown_all(&self) -> Result<()> {
        self.request(
            HolderMessageType::ShutdownAll,
            HolderMessageType::ShutdownAllResp,
            Vec::new(),
        )?;
        Ok(())
    }

    fn discard_events_through(&self, generation: u64) {
        self.events
            .lock()
            .unwrap()
            .retain(|event| event.generation > generation);
    }

    pub(crate) fn wait_for_session_exit(&self, session_id: u32) -> Result<ExitContext> {
        let environment_context = self.has_capability(HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT);
        loop {
            if let Some(context) = take_exit_event(
                &mut self.events.lock().unwrap(),
                session_id,
                environment_context,
            ) {
                return Ok(context);
            }
            let event = {
                let mut stream = self.stream.lock().unwrap();
                read_frame_with_minor(&mut stream, self.protocol_minor())?
            };
            if event.request_id != 0 || !is_event(event.message_type) {
                return Err(PersistError::invalid_argument(
                    "unexpected holder frame while waiting for exit",
                ));
            }
            validate_event(&event, environment_context)?;
            self.observe_generation(event.generation)?;
            if event.message_type == HolderMessageType::SessionExited {
                let exited = decode_exit_event(&event.payload, environment_context)?;
                if exited.session_id == session_id {
                    return Ok(exited);
                }
            }
            let mut events = self.events.lock().unwrap();
            if events.len() >= MAX_PENDING_EVENTS {
                return Err(PersistError::invalid_argument(
                    "holder control event queue limit exceeded",
                ));
            }
            events.push_back(event);
        }
    }

    fn operation(
        &self,
        request_type: HolderMessageType,
        response_type: HolderMessageType,
        session_id: u32,
    ) -> Result<()> {
        let frame = self.request(
            request_type,
            response_type,
            encode_operation_request(&OperationRequest { session_id }),
        )?;
        require_operation_ok(&frame.payload, "operation")
    }

    fn request(
        &self,
        request_type: HolderMessageType,
        response_type: HolderMessageType,
        payload: Vec<u8>,
    ) -> Result<HolderFrame> {
        self.request_with_minor(request_type, response_type, payload, self.protocol_minor())
    }

    fn request_with_minor(
        &self,
        request_type: HolderMessageType,
        response_type: HolderMessageType,
        payload: Vec<u8>,
        minor: u16,
    ) -> Result<HolderFrame> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        if request_id == 0 {
            return Err(PersistError::internal_error(
                "holder request ID space exhausted",
            ));
        }
        let generation = self.generation.load(Ordering::Acquire);
        let mut stream = self.stream.lock().unwrap();
        write_frame_with_minor(
            &mut stream,
            &HolderFrame {
                message_type: request_type,
                flags: 0,
                request_id,
                generation,
                payload,
            },
            minor,
        )?;
        loop {
            let response = read_frame_with_minor(&mut stream, minor)?;
            if response.request_id == request_id && response.message_type == response_type {
                self.observe_generation(response.generation)?;
                return Ok(response);
            }
            if response.request_id != 0 || !is_event(response.message_type) {
                return Err(PersistError::invalid_argument(
                    "holder response does not match request",
                ));
            }
            validate_event(
                &response,
                self.has_capability(HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT),
            )?;
            self.observe_generation(response.generation)?;
            let mut events = self.events.lock().unwrap();
            if events.len() >= MAX_PENDING_EVENTS {
                return Err(PersistError::invalid_argument(
                    "holder control event queue limit exceeded",
                ));
            }
            events.push_back(response);
        }
    }

    fn observe_generation(&self, generation: u64) -> Result<()> {
        let prior = self.generation.load(Ordering::Acquire);
        if generation < prior {
            return Err(PersistError::invalid_argument(
                "holder response generation moved backwards",
            ));
        }
        self.generation.store(generation, Ordering::Release);
        Ok(())
    }
}

fn configure_stream(stream: &UnixStream) -> Result<()> {
    stream
        .set_read_timeout(Some(CONTROL_TIMEOUT))
        .and_then(|_| stream.set_write_timeout(Some(CONTROL_TIMEOUT)))
        .map_err(|source| io_error("configure holder control timeout", source))
}

fn write_frame(stream: &mut UnixStream, frame: &HolderFrame) -> Result<()> {
    write_frame_with_minor(stream, frame, HOLDER_PROTOCOL_BASELINE_MINOR)
}

fn write_frame_with_minor(stream: &mut UnixStream, frame: &HolderFrame, minor: u16) -> Result<()> {
    let bytes =
        encode_frame_with_minor(frame, minor).map_err(|error| protocol_error("request", error))?;
    stream
        .write_all(&bytes)
        .map_err(|source| io_error("write holder control frame", source))
}

fn read_frame(stream: &mut UnixStream) -> Result<HolderFrame> {
    read_frame_with_minor(stream, HOLDER_PROTOCOL_BASELINE_MINOR)
}

fn read_frame_with_minor(stream: &mut UnixStream, minor: u16) -> Result<HolderFrame> {
    let mut header = [0u8; HOLDER_HEADER_SIZE];
    stream
        .read_exact(&mut header)
        .map_err(|source| io_error("read holder control header", source))?;
    let payload_len = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    if payload_len > MAX_HOLDER_CONTROL_FRAME {
        return Err(PersistError::invalid_argument(
            "holder control payload exceeds limit",
        ));
    }
    let mut bytes = header.to_vec();
    let mut payload = vec![0; payload_len];
    stream
        .read_exact(&mut payload)
        .map_err(|source| io_error("read holder control payload", source))?;
    bytes.extend(payload);
    decode_frame_with_minor(&bytes, minor).map_err(|error| protocol_error("response", error))
}

fn validate_response(frame: &HolderFrame, id: u32, kind: HolderMessageType) -> Result<()> {
    if frame.request_id != id || frame.message_type != kind || frame.flags != 0 {
        return Err(PersistError::invalid_argument(
            "holder response does not match request",
        ));
    }
    Ok(())
}

fn is_event(kind: HolderMessageType) -> bool {
    matches!(
        kind,
        HolderMessageType::SessionStarted
            | HolderMessageType::SessionExited
            | HolderMessageType::WriterChanged
            | HolderMessageType::LogDegraded
    )
}

fn validate_event(frame: &HolderFrame, environment_context: bool) -> Result<()> {
    let valid = match frame.message_type {
        HolderMessageType::SessionStarted | HolderMessageType::WriterChanged => {
            decode_operation_request(&frame.payload).is_ok()
        }
        HolderMessageType::SessionExited => {
            if environment_context {
                decode_session_exited_event_v2(&frame.payload).is_ok()
            } else {
                decode_session_exited_event(&frame.payload).is_ok()
            }
        }
        HolderMessageType::LogDegraded => decode_log_degraded(&frame.payload).is_ok(),
        _ => false,
    };
    if !valid || frame.flags != 0 {
        return Err(PersistError::invalid_argument(
            "invalid holder control event",
        ));
    }
    Ok(())
}

fn require_operation_ok(payload: &[u8], context: &str) -> Result<()> {
    let response =
        decode_operation_response(payload).map_err(|error| protocol_error(context, error))?;
    if response.status != OperationStatus::Ok {
        return Err(PersistError::invalid_argument(format!(
            "holder {context} failed: {:?}: {}",
            response.status, response.message
        )));
    }
    Ok(())
}

fn take_exit_event(
    events: &mut VecDeque<HolderFrame>,
    session_id: u32,
    environment_context: bool,
) -> Option<ExitContext> {
    let position = events.iter().position(|frame| {
        frame.message_type == HolderMessageType::SessionExited
            && decode_exit_event(&frame.payload, environment_context)
                .is_ok_and(|event| event.session_id == session_id)
    })?;
    let frame = events.remove(position)?;
    decode_exit_event(&frame.payload, environment_context).ok()
}

fn decode_exit_event(payload: &[u8], environment_context: bool) -> Result<ExitContext> {
    if environment_context {
        let event = decode_session_exited_event_v2(payload)
            .map_err(|error| protocol_error("SessionExited v2", error))?;
        Ok(ExitContext {
            session_id: event.session_id,
            exit_code: event.exit_code,
            cwd: event.cwd,
            environment: event.environment,
        })
    } else {
        let event = decode_session_exited_event(payload)
            .map_err(|error| protocol_error("SessionExited", error))?;
        Ok(ExitContext {
            session_id: event.session_id,
            exit_code: event.exit_code,
            cwd: event.cwd,
            environment: None,
        })
    }
}

fn random_nonce() -> Result<[u8; 16]> {
    let mut file = File::open("/dev/urandom")
        .map_err(|source| io_error("open urandom for holder nonce", source))?;
    let mut nonce = [0; 16];
    file.read_exact(&mut nonce)
        .map_err(|source| io_error("read holder nonce", source))?;
    if nonce == [0; 16] {
        return Err(PersistError::internal_error("generated zero holder nonce"));
    }
    Ok(nonce)
}

fn protocol_error(context: &str, error: HolderProtocolError) -> PersistError {
    PersistError::invalid_argument(format!("invalid holder {context}: {error:?}"))
}

fn io_error(operation: &'static str, source: std::io::Error) -> PersistError {
    PersistError::Io { operation, source }
}
use std::collections::VecDeque;
