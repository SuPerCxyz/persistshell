mod capability;
mod frame;
mod handshake;
mod inventory;
mod operation;
mod session;
mod wire;

pub use capability::{
    decode_capability_request, decode_capability_response, encode_capability_request,
    encode_capability_response,
};
pub use frame::{
    decode_frame, decode_frame_with_minor, encode_frame, encode_frame_with_minor,
    HolderFrameAccumulator,
};
pub use handshake::{
    decode_control_hello, decode_control_hello_ack, decode_data_hello, decode_data_hello_ack,
    encode_control_hello, encode_control_hello_ack, encode_data_hello, encode_data_hello_ack,
};
pub use inventory::{
    decode_inventory_request, decode_inventory_response, encode_inventory_request,
    encode_inventory_response,
};
pub use operation::{
    decode_exit_context_response, decode_exit_context_response_v2, decode_log_degraded,
    decode_operation_request, decode_operation_response, decode_resize_request,
    decode_session_exited_event, decode_session_exited_event_v2, decode_signal_request,
    encode_exit_context_response, encode_exit_context_response_v2, encode_log_degraded,
    encode_operation_request, encode_operation_response, encode_resize_request,
    encode_session_exited_event, encode_session_exited_event_v2, encode_signal_request,
    ExitContextResponse, ExitContextResponseV2, LogDegradedEvent, OperationRequest,
    OperationResponse, OperationStatus, ResizeRequest, SessionExitedEvent, SessionExitedEventV2,
    SignalRequest,
};
pub use session::{
    decode_attach_request, decode_create_request, decode_create_request_v2, encode_attach_request,
    encode_create_request, encode_create_request_v2,
};

pub const HOLDER_MAGIC: [u8; 4] = *b"PSHH";
pub const HOLDER_PROTOCOL_MAJOR: u16 = 1;
pub const HOLDER_PROTOCOL_BASELINE_MINOR: u16 = 1;
pub const HOLDER_PROTOCOL_MINOR: u16 = 2;
pub const HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT: u64 = 1;
pub const HOLDER_HEADER_SIZE: usize = 32;
pub const MAX_HOLDER_CONTROL_FRAME: usize = 1024 * 1024;
pub const MAX_HOLDER_IO_FRAME: usize = 64 * 1024;
pub const MAX_INVENTORY_ENTRIES: u16 = 256;
pub const MAX_HOLDER_PATH: usize = 4096;
pub const MAX_HOLDER_ENV_VARS: usize = 128;
pub const MAX_HOLDER_ENV_NAME: usize = 128;
pub const MAX_HOLDER_ENV_VALUE: usize = 8192;
pub const MAX_HOLDER_ARGUMENTS: usize = 16;
pub const MAX_HOLDER_ARGUMENT: usize = 4096;
pub const MAX_HOLDER_RING_BUFFER: u32 = 64 * 1024 * 1024;
pub const MAX_HOLDER_ERROR_MESSAGE: usize = 1024;
pub const MAX_HOLDER_ACCUMULATOR: usize = MAX_HOLDER_CONTROL_FRAME + HOLDER_HEADER_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderProtocolError {
    Truncated,
    Trailing,
    InvalidMagic,
    VersionMismatch,
    UnknownMessageType,
    PayloadTooLarge,
    InvalidField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum HolderMessageType {
    ControlHello = 0x0001,
    ControlHelloAck = 0x0002,
    Error = 0x0003,
    Capability = 0x0004,
    CapabilityResp = 0x0005,
    Inventory = 0x0010,
    InventoryResp = 0x0011,
    Create = 0x0012,
    CreateResp = 0x0013,
    Close = 0x0014,
    CloseResp = 0x0015,
    Kill = 0x0016,
    KillResp = 0x0017,
    ShutdownAll = 0x0018,
    ShutdownAllResp = 0x0019,
    GetExitContext = 0x001a,
    GetExitContextResp = 0x001b,
    RetireExited = 0x001c,
    RetireExitedResp = 0x001d,
    SessionStarted = 0x0020,
    SessionExited = 0x0021,
    WriterChanged = 0x0022,
    LogDegraded = 0x0023,
    DataHello = 0x0100,
    DataHelloAck = 0x0101,
    Attach = 0x0110,
    AttachResp = 0x0111,
    Detach = 0x0112,
    Input = 0x0120,
    Output = 0x0121,
    Resize = 0x0122,
    Signal = 0x0123,
    WriteGranted = 0x0124,
    WriteRevoked = 0x0125,
}

impl HolderMessageType {
    fn from_u16(value: u16) -> Option<Self> {
        Some(match value {
            0x0001 => Self::ControlHello,
            0x0002 => Self::ControlHelloAck,
            0x0003 => Self::Error,
            0x0004 => Self::Capability,
            0x0005 => Self::CapabilityResp,
            0x0010 => Self::Inventory,
            0x0011 => Self::InventoryResp,
            0x0012 => Self::Create,
            0x0013 => Self::CreateResp,
            0x0014 => Self::Close,
            0x0015 => Self::CloseResp,
            0x0016 => Self::Kill,
            0x0017 => Self::KillResp,
            0x0018 => Self::ShutdownAll,
            0x0019 => Self::ShutdownAllResp,
            0x001a => Self::GetExitContext,
            0x001b => Self::GetExitContextResp,
            0x001c => Self::RetireExited,
            0x001d => Self::RetireExitedResp,
            0x0020 => Self::SessionStarted,
            0x0021 => Self::SessionExited,
            0x0022 => Self::WriterChanged,
            0x0023 => Self::LogDegraded,
            0x0100 => Self::DataHello,
            0x0101 => Self::DataHelloAck,
            0x0110 => Self::Attach,
            0x0111 => Self::AttachResp,
            0x0112 => Self::Detach,
            0x0120 => Self::Input,
            0x0121 => Self::Output,
            0x0122 => Self::Resize,
            0x0123 => Self::Signal,
            0x0124 => Self::WriteGranted,
            0x0125 => Self::WriteRevoked,
            _ => return None,
        })
    }

    fn max_payload(self) -> usize {
        match self {
            Self::Input | Self::Output => MAX_HOLDER_IO_FRAME,
            _ => MAX_HOLDER_CONTROL_FRAME,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HolderFrame {
    pub message_type: HolderMessageType,
    pub flags: u16,
    pub request_id: u32,
    pub generation: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelloStatus {
    Accepted,
    VersionMismatch,
    PermissionDenied,
    Busy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlHello {
    pub uid: u32,
    pub daemon_pid: u32,
    pub nonce: [u8; 16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlHelloAck {
    pub holder_pid: u32,
    pub instance_id: [u8; 16],
    pub nonce: [u8; 16],
    pub status: HelloStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityRequest {
    pub instance_id: [u8; 16],
    pub nonce: [u8; 16],
    pub max_minor: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityResponse {
    pub instance_id: [u8; 16],
    pub nonce: [u8; 16],
    pub selected_minor: u16,
    pub capabilities: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataHello {
    pub daemon_pid: u32,
    pub instance_id: [u8; 16],
    pub nonce: [u8; 16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataHelloAck {
    pub instance_id: [u8; 16],
    pub nonce: [u8; 16],
    pub status: HelloStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryRequest {
    pub cursor: u32,
    pub limit: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderSessionState {
    Running,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderLogState {
    Healthy,
    Degraded,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HolderSessionEntry {
    pub session_id: u32,
    pub shell_pid: u32,
    pub state: HolderSessionState,
    pub exit_code: Option<i32>,
    pub created_at_ms: u64,
    pub last_active_at_ms: u64,
    pub ring_bytes: u32,
    pub writer_active: bool,
    pub log_state: HolderLogState,
    pub exit_context_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryResponse {
    pub entries: Vec<HolderSessionEntry>,
    pub next_cursor: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateSessionRequest {
    pub session_id: u32,
    pub shell: String,
    pub arguments: Vec<String>,
    pub cwd: Option<String>,
    pub launch_environment: persist_core::shell_state::ShellLaunchEnvironment,
    pub history_file: Option<String>,
    pub ring_buffer_size: u32,
    pub log_path: Option<String>,
    pub state_file: String,
    pub state_incarnation: [u8; 16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderAttachMode {
    ReadWrite,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachRequest {
    pub session_id: u32,
    pub mode: HolderAttachMode,
    pub replay_bytes: u32,
}

#[cfg(test)]
mod frame_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_m54;
