use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use persist_core::{PersistError, Result};

pub const MAX_CONTROL_FRAME: usize = 1024 * 1024;
pub const MAX_IO_FRAME: usize = 64 * 1024;

pub const HEADER_SIZE: usize = 12;
pub const STDIN_FRAME_MAX: usize = MAX_IO_FRAME;
pub const STDOUT_FRAME_MAX: usize = MAX_IO_FRAME;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    Hello = 0x0001,
    HelloAck = 0x0002,
    Error = 0x0003,
    NewSession = 0x0010,
    NewSessionResp = 0x0011,
    ListSessions = 0x0012,
    ListSessionsResp = 0x0013,
    Attach = 0x0014,
    AttachResp = 0x0015,
    Detach = 0x0016,
    Rename = 0x0017,
    RenameResp = 0x0018,
    DetachSignal = 0x0019,
    DetachSignalResp = 0x001A,
    Signal = 0x001B,
    SignalResp = 0x001C,
    NoteSet = 0x001D,
    NoteSetResp = 0x001E,
    NoteGet = 0x001F,
    NoteGetResp = 0x0020,
    TagAdd = 0x0021,
    TagAddResp = 0x0022,
    TagRemove = 0x0023,
    TagRemoveResp = 0x0024,
    TagList = 0x0025,
    TagListResp = 0x0026,
    ListSessionsByTag = 0x0027,
    PinSet = 0x0028,
    PinSetResp = 0x0029,
    AttachReadOnly = 0x002A,
    WriteRequest = 0x002B,
    WriteGranted = 0x002C,
    WriteRevoked = 0x002D,
    LockSet = 0x002E,
    LockSetResp = 0x002F,
    ProcessTree = 0x0030,
    ProcessTreeResp = 0x0031,
    ProcessStats = 0x0032,
    ProcessStatsResp = 0x0033,
    SessionSnapshot = 0x0034,
    SessionSnapshotResp = 0x0035,
    Metrics = 0x0036,
    MetricsResp = 0x0037,
    DashboardSummary = 0x0038,
    DashboardSummaryResp = 0x0039,
    DashboardTrend = 0x003A,
    DashboardTrendResp = 0x003B,
    Close = 0x0302,
    CloseResp = 0x0304,
    Kill = 0x0303,
    KillResp = 0x0305,
    Ping = 0x0300,
    Pong = 0x0301,
    Stdin = 0x0100,
    Stdout = 0x0101,
    Resize = 0x0102,
    SessionExited = 0x0103,
}

impl MessageType {
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x0001 => Some(Self::Hello),
            0x0002 => Some(Self::HelloAck),
            0x0003 => Some(Self::Error),
            0x0010 => Some(Self::NewSession),
            0x0011 => Some(Self::NewSessionResp),
            0x0012 => Some(Self::ListSessions),
            0x0013 => Some(Self::ListSessionsResp),
            0x0014 => Some(Self::Attach),
            0x0015 => Some(Self::AttachResp),
            0x0016 => Some(Self::Detach),
            0x0017 => Some(Self::Rename),
            0x0018 => Some(Self::RenameResp),
            0x0019 => Some(Self::DetachSignal),
            0x001A => Some(Self::DetachSignalResp),
            0x001B => Some(Self::Signal),
            0x001C => Some(Self::SignalResp),
            0x001D => Some(Self::NoteSet),
            0x001E => Some(Self::NoteSetResp),
            0x001F => Some(Self::NoteGet),
            0x0020 => Some(Self::NoteGetResp),
            0x0021 => Some(Self::TagAdd),
            0x0022 => Some(Self::TagAddResp),
            0x0023 => Some(Self::TagRemove),
            0x0024 => Some(Self::TagRemoveResp),
            0x0025 => Some(Self::TagList),
            0x0026 => Some(Self::TagListResp),
            0x0027 => Some(Self::ListSessionsByTag),
            0x0028 => Some(Self::PinSet),
            0x0029 => Some(Self::PinSetResp),
            0x002A => Some(Self::AttachReadOnly),
            0x002B => Some(Self::WriteRequest),
            0x002C => Some(Self::WriteGranted),
            0x002D => Some(Self::WriteRevoked),
            0x002E => Some(Self::LockSet),
            0x002F => Some(Self::LockSetResp),
            0x0030 => Some(Self::ProcessTree),
            0x0031 => Some(Self::ProcessTreeResp),
            0x0032 => Some(Self::ProcessStats),
            0x0033 => Some(Self::ProcessStatsResp),
            0x0034 => Some(Self::SessionSnapshot),
            0x0035 => Some(Self::SessionSnapshotResp),
            0x0036 => Some(Self::Metrics),
            0x0037 => Some(Self::MetricsResp),
            0x0038 => Some(Self::DashboardSummary),
            0x0039 => Some(Self::DashboardSummaryResp),
            0x003A => Some(Self::DashboardTrend),
            0x003B => Some(Self::DashboardTrendResp),
            0x0100 => Some(Self::Stdin),
            0x0101 => Some(Self::Stdout),
            0x0102 => Some(Self::Resize),
            0x0103 => Some(Self::SessionExited),
            0x0300 => Some(Self::Ping),
            0x0301 => Some(Self::Pong),
            0x0302 => Some(Self::Close),
            0x0303 => Some(Self::Kill),
            0x0304 => Some(Self::CloseResp),
            0x0305 => Some(Self::KillResp),
            _ => None,
        }
    }

    pub fn is_io_message(&self) -> bool {
        matches!(self, Self::Stdin | Self::Stdout | Self::Resize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelloStatus {
    Accepted,
    VersionMismatch,
    PermissionDenied,
}

#[derive(Debug, Clone)]
pub struct HelloPayload {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub uid: u32,
    pub pid: u32,
}

#[derive(Debug, Clone)]
pub struct HelloAckPayload {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub pid: u32,
    pub status: HelloStatus,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub msg_type: MessageType,
    pub flags: u16,
    pub request_id: u32,
    pub payload: Vec<u8>,
}

pub fn write_frame(stream: &mut UnixStream, frame: &Frame) -> Result<()> {
    let mut header = Vec::with_capacity(HEADER_SIZE);
    header.extend_from_slice(&(frame.payload.len() as u32).to_be_bytes());
    header.extend_from_slice(&(frame.msg_type as u16).to_be_bytes());
    header.extend_from_slice(&frame.flags.to_be_bytes());
    header.extend_from_slice(&frame.request_id.to_be_bytes());

    stream
        .write_all(&header)
        .map_err(|source| PersistError::Io {
            operation: "write frame header",
            source,
        })?;
    if !frame.payload.is_empty() {
        stream
            .write_all(&frame.payload)
            .map_err(|source| PersistError::Io {
                operation: "write frame payload",
                source,
            })?;
    }
    Ok(())
}

pub fn read_frame(stream: &mut UnixStream) -> Result<Frame> {
    let header_buf = read_n(stream, HEADER_SIZE).map_err(|source| PersistError::Io {
        operation: "read frame header",
        source,
    })?;

    let payload_len = u32::from_be_bytes(header_buf[0..4].try_into().unwrap()) as usize;
    let raw_type = u16::from_be_bytes(header_buf[4..6].try_into().unwrap());
    let flags = u16::from_be_bytes(header_buf[6..8].try_into().unwrap());
    let request_id = u32::from_be_bytes(header_buf[8..12].try_into().unwrap());

    if payload_len > MAX_CONTROL_FRAME {
        return Err(PersistError::invalid_argument(format!(
            "frame payload too large: {payload_len} > {MAX_CONTROL_FRAME}"
        )));
    }

    let msg_type = MessageType::from_u16(raw_type).ok_or_else(|| {
        PersistError::invalid_argument(format!("unknown message type: 0x{raw_type:04X}"))
    })?;

    let payload = if payload_len > 0 {
        read_n(stream, payload_len).map_err(|source| PersistError::Io {
            operation: "read frame payload",
            source,
        })?
    } else {
        Vec::new()
    };

    Ok(Frame {
        msg_type,
        flags,
        request_id,
        payload,
    })
}

fn read_n(stream: &mut UnixStream, n: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    let mut offset = 0;
    while offset < n {
        let count = stream.read(&mut buf[offset..])?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "socket closed before frame complete",
            ));
        }
        offset += count;
    }
    Ok(buf)
}

pub fn encode_hello(hello: &HelloPayload) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16);
    buf.extend_from_slice(&(hello.protocol_major as u32).to_be_bytes());
    buf.extend_from_slice(&(hello.protocol_minor as u32).to_be_bytes());
    buf.extend_from_slice(&hello.uid.to_be_bytes());
    buf.extend_from_slice(&hello.pid.to_be_bytes());
    buf
}

pub fn decode_hello(payload: &[u8]) -> Option<HelloPayload> {
    if payload.len() < 16 {
        return None;
    }
    let protocol_major = u32::from_be_bytes(payload[0..4].try_into().ok()?) as u16;
    let protocol_minor = u32::from_be_bytes(payload[4..8].try_into().ok()?) as u16;
    let uid = u32::from_be_bytes(payload[8..12].try_into().ok()?);
    let pid = u32::from_be_bytes(payload[12..16].try_into().ok()?);

    Some(HelloPayload {
        protocol_major,
        protocol_minor,
        uid,
        pid,
    })
}

pub fn encode_hello_ack(ack: &HelloAckPayload) -> Vec<u8> {
    let mut buf = Vec::with_capacity(14);
    buf.extend_from_slice(&(ack.protocol_major as u32).to_be_bytes());
    buf.extend_from_slice(&(ack.protocol_minor as u32).to_be_bytes());
    buf.extend_from_slice(&ack.pid.to_be_bytes());
    buf.extend_from_slice(&(ack.status as u16).to_be_bytes());
    buf
}

pub fn decode_hello_ack(payload: &[u8]) -> Option<HelloAckPayload> {
    if payload.len() < 14 {
        return None;
    }
    let protocol_major = u32::from_be_bytes(payload[0..4].try_into().ok()?) as u16;
    let protocol_minor = u32::from_be_bytes(payload[4..8].try_into().ok()?) as u16;
    let pid = u32::from_be_bytes(payload[8..12].try_into().ok()?);
    let status = match u16::from_be_bytes(payload[12..14].try_into().ok()?) {
        0 => HelloStatus::Accepted,
        1 => HelloStatus::VersionMismatch,
        2 => HelloStatus::PermissionDenied,
        _ => return None,
    };

    Some(HelloAckPayload {
        protocol_major,
        protocol_minor,
        pid,
        status,
    })
}

// ── Session management payloads ──

#[derive(Debug, Clone)]
pub struct NewSessionRespPayload {
    pub session_id: u32,
    pub name: String,
}

pub fn encode_new_session_resp(p: &NewSessionRespPayload) -> Vec<u8> {
    let mut buf = Vec::with_capacity(6 + p.name.len());
    buf.extend_from_slice(&p.session_id.to_be_bytes());
    encode_str16(&mut buf, &p.name);
    buf
}

pub fn decode_new_session_resp(data: &[u8]) -> Option<NewSessionRespPayload> {
    if data.len() < 6 {
        return None;
    }
    let session_id = u32::from_be_bytes(data[0..4].try_into().ok()?);
    let (name, _) = decode_str16(data, 4)?;
    Some(NewSessionRespPayload { session_id, name })
}

#[derive(Debug, Clone)]
pub struct AttachPayload {
    pub session_id: u32,
}

pub fn encode_attach(p: &AttachPayload) -> Vec<u8> {
    p.session_id.to_be_bytes().to_vec()
}

pub fn decode_attach(data: &[u8]) -> Option<AttachPayload> {
    if data.len() < 4 {
        return None;
    }
    Some(AttachPayload {
        session_id: u32::from_be_bytes(data[0..4].try_into().ok()?),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriterControlPayload {
    pub session_id: u32,
}

pub fn encode_writer_control(p: &WriterControlPayload) -> Vec<u8> {
    p.session_id.to_be_bytes().to_vec()
}

pub fn decode_writer_control(data: &[u8]) -> Option<WriterControlPayload> {
    if data.len() != 4 {
        return None;
    }
    Some(WriterControlPayload {
        session_id: u32::from_be_bytes(data.try_into().ok()?),
    })
}

#[derive(Debug, Clone)]
pub struct AttachRespPayload {
    pub ok: bool,
    pub error_msg: String,
}

pub fn encode_attach_resp(p: &AttachRespPayload) -> Vec<u8> {
    let mut buf = Vec::with_capacity(5 + p.error_msg.len());
    buf.push(p.ok as u8);
    let msg_bytes = p.error_msg.as_bytes();
    let len = msg_bytes.len().min(u16::MAX as usize) as u16;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&msg_bytes[..len as usize]);
    buf
}

pub fn decode_attach_resp(data: &[u8]) -> Option<AttachRespPayload> {
    if data.len() < 3 {
        return None;
    }
    let ok = data[0] != 0;
    let msg_len = u16::from_be_bytes(data[1..3].try_into().ok()?) as usize;
    let error_msg = if msg_len > 0 && data.len() >= 3 + msg_len {
        String::from_utf8_lossy(&data[3..3 + msg_len]).to_string()
    } else {
        String::new()
    };
    Some(AttachRespPayload { ok, error_msg })
}

#[derive(Debug, Clone)]
pub struct DetachPayload {
    pub session_id: u32,
}

pub fn encode_detach(p: &DetachPayload) -> Vec<u8> {
    p.session_id.to_be_bytes().to_vec()
}

pub fn decode_detach(data: &[u8]) -> Option<DetachPayload> {
    if data.len() < 4 {
        return None;
    }
    Some(DetachPayload {
        session_id: u32::from_be_bytes(data[0..4].try_into().ok()?),
    })
}

#[derive(Debug, Clone)]
pub struct RenamePayload {
    pub session_id: u32,
    pub name: String,
}

pub fn encode_rename(p: &RenamePayload) -> Vec<u8> {
    let name_bytes = p.name.as_bytes();
    let name_len = name_bytes.len().min(u16::MAX as usize) as u16;
    let mut buf = Vec::with_capacity(6 + name_len as usize);
    buf.extend_from_slice(&p.session_id.to_be_bytes());
    buf.extend_from_slice(&name_len.to_be_bytes());
    buf.extend_from_slice(&name_bytes[..name_len as usize]);
    buf
}

pub fn decode_rename(data: &[u8]) -> Option<RenamePayload> {
    if data.len() < 6 {
        return None;
    }
    let session_id = u32::from_be_bytes(data[0..4].try_into().ok()?);
    let name_len = u16::from_be_bytes(data[4..6].try_into().ok()?) as usize;
    let name = if name_len > 0 && data.len() >= 6 + name_len {
        String::from_utf8_lossy(&data[6..6 + name_len]).to_string()
    } else {
        String::new()
    };
    Some(RenamePayload { session_id, name })
}

#[derive(Debug, Clone)]
pub struct NotePayload {
    pub session_id: u32,
    pub note: String,
}

pub fn encode_note(p: &NotePayload) -> Vec<u8> {
    let note_bytes = p.note.as_bytes();
    let note_len = note_bytes.len().min(u16::MAX as usize) as u16;
    let mut buf = Vec::with_capacity(6 + note_len as usize);
    buf.extend_from_slice(&p.session_id.to_be_bytes());
    buf.extend_from_slice(&note_len.to_be_bytes());
    buf.extend_from_slice(&note_bytes[..note_len as usize]);
    buf
}

pub fn decode_note(data: &[u8]) -> Option<NotePayload> {
    if data.len() < 6 {
        return None;
    }
    let session_id = u32::from_be_bytes(data[0..4].try_into().ok()?);
    let note_len = u16::from_be_bytes(data[4..6].try_into().ok()?) as usize;
    let note = if note_len > 0 && data.len() >= 6 + note_len {
        String::from_utf8_lossy(&data[6..6 + note_len]).to_string()
    } else {
        String::new()
    };
    Some(NotePayload { session_id, note })
}

pub fn encode_note_get_resp(note: &str) -> Vec<u8> {
    let bytes = note.as_bytes();
    let len = bytes.len().min(u16::MAX as usize) as u16;
    let mut buf = Vec::with_capacity(2 + len as usize);
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&bytes[..len as usize]);
    buf
}

pub fn decode_note_get_resp(data: &[u8]) -> Option<String> {
    if data.len() < 2 {
        return None;
    }
    let len = u16::from_be_bytes(data[0..2].try_into().ok()?) as usize;
    if data.len() < 2 + len {
        return None;
    }
    Some(String::from_utf8_lossy(&data[2..2 + len]).to_string())
}

#[derive(Debug, Clone)]
pub struct TagPayload {
    pub session_id: u32,
    pub tag: String,
}

pub fn encode_tag(p: &TagPayload) -> Vec<u8> {
    let tag_bytes = p.tag.as_bytes();
    let tag_len = tag_bytes.len().min(u16::MAX as usize) as u16;
    let mut buf = Vec::with_capacity(6 + tag_len as usize);
    buf.extend_from_slice(&p.session_id.to_be_bytes());
    buf.extend_from_slice(&tag_len.to_be_bytes());
    buf.extend_from_slice(&tag_bytes[..tag_len as usize]);
    buf
}

pub fn decode_tag(data: &[u8]) -> Option<TagPayload> {
    if data.len() < 6 {
        return None;
    }
    let session_id = u32::from_be_bytes(data[0..4].try_into().ok()?);
    let tag_len = u16::from_be_bytes(data[4..6].try_into().ok()?) as usize;
    let tag = if tag_len > 0 && data.len() >= 6 + tag_len {
        String::from_utf8_lossy(&data[6..6 + tag_len]).to_string()
    } else {
        String::new()
    };
    Some(TagPayload { session_id, tag })
}

#[derive(Debug, Clone)]
pub struct PinPayload {
    pub session_id: u32,
    pub pinned: bool,
}

#[derive(Debug, Clone)]
pub struct LockPayload {
    pub session_id: u32,
    pub locked: bool,
}

pub fn encode_lock(p: &LockPayload) -> Vec<u8> {
    let mut buf = p.session_id.to_be_bytes().to_vec();
    buf.push(p.locked as u8);
    buf
}

pub fn decode_lock(data: &[u8]) -> Option<LockPayload> {
    if data.len() != 5 {
        return None;
    }
    Some(LockPayload {
        session_id: u32::from_be_bytes(data[0..4].try_into().ok()?),
        locked: data[4] != 0,
    })
}

pub fn encode_pin(p: &PinPayload) -> Vec<u8> {
    let mut buf = Vec::with_capacity(5);
    buf.extend_from_slice(&p.session_id.to_be_bytes());
    buf.push(if p.pinned { 1 } else { 0 });
    buf
}

pub fn decode_pin(data: &[u8]) -> Option<PinPayload> {
    if data.len() < 5 {
        return None;
    }
    let session_id = u32::from_be_bytes(data[0..4].try_into().ok()?);
    let pinned = data[4] != 0;
    Some(PinPayload { session_id, pinned })
}

#[derive(Debug, Clone)]
pub struct TagListRespPayload {
    pub tags: Vec<String>,
}

pub fn encode_tag_list_resp(p: &TagListRespPayload) -> Vec<u8> {
    let mut buf = Vec::new();
    let count = p.tags.len().min(u16::MAX as usize) as u16;
    buf.extend_from_slice(&count.to_be_bytes());
    for tag in &p.tags {
        let tag_bytes = tag.as_bytes();
        let tag_len = tag_bytes.len().min(u16::MAX as usize) as u16;
        buf.extend_from_slice(&tag_len.to_be_bytes());
        buf.extend_from_slice(&tag_bytes[..tag_len as usize]);
    }
    buf
}

pub fn decode_tag_list_resp(data: &[u8]) -> Option<TagListRespPayload> {
    if data.len() < 2 {
        return None;
    }
    let count = u16::from_be_bytes(data[0..2].try_into().ok()?) as usize;
    let mut offset = 2;
    let mut tags = Vec::with_capacity(count);
    for _ in 0..count {
        if offset + 2 > data.len() {
            return None;
        }
        let tag_len = u16::from_be_bytes(data[offset..offset + 2].try_into().ok()?) as usize;
        offset += 2;
        if offset + tag_len > data.len() {
            return None;
        }
        let tag = String::from_utf8_lossy(&data[offset..offset + tag_len]).to_string();
        offset += tag_len;
        tags.push(tag);
    }
    Some(TagListRespPayload { tags })
}

// Close and Kill share the same session_id payload as Detach.
// Responses for both use OpRespPayload (bool + error_msg).

#[derive(Debug, Clone)]
pub struct OpRespPayload {
    pub ok: bool,
    pub error_msg: String,
}

pub fn encode_op_resp(p: &OpRespPayload) -> Vec<u8> {
    let mut buf = Vec::with_capacity(5 + p.error_msg.len());
    buf.push(p.ok as u8);
    let msg_bytes = p.error_msg.as_bytes();
    let len = msg_bytes.len().min(u16::MAX as usize) as u16;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&msg_bytes[..len as usize]);
    buf
}

pub fn decode_op_resp(data: &[u8]) -> Option<OpRespPayload> {
    if data.len() < 3 {
        return None;
    }
    let ok = data[0] != 0;
    let msg_len = u16::from_be_bytes(data[1..3].try_into().ok()?) as usize;
    let error_msg = if msg_len > 0 && data.len() >= 3 + msg_len {
        String::from_utf8_lossy(&data[3..3 + msg_len]).to_string()
    } else {
        String::new()
    };
    Some(OpRespPayload { ok, error_msg })
}

#[derive(Debug, Clone)]
pub struct ResizePayload {
    pub rows: u16,
    pub cols: u16,
}

pub fn encode_resize(p: &ResizePayload) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4);
    buf.extend_from_slice(&p.rows.to_be_bytes());
    buf.extend_from_slice(&p.cols.to_be_bytes());
    buf
}

pub fn decode_resize(data: &[u8]) -> Option<ResizePayload> {
    if data.len() < 4 {
        return None;
    }
    Some(ResizePayload {
        rows: u16::from_be_bytes(data[0..2].try_into().ok()?),
        cols: u16::from_be_bytes(data[2..4].try_into().ok()?),
    })
}

#[derive(Debug, Clone)]
pub struct SignalPayload {
    pub session_id: u32,
    pub signal: u32,
}

pub fn encode_signal(p: &SignalPayload) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend_from_slice(&p.session_id.to_be_bytes());
    buf.extend_from_slice(&p.signal.to_be_bytes());
    buf
}

pub fn decode_signal(data: &[u8]) -> Option<SignalPayload> {
    if data.len() < 8 {
        return None;
    }
    Some(SignalPayload {
        session_id: u32::from_be_bytes(data[0..4].try_into().ok()?),
        signal: u32::from_be_bytes(data[4..8].try_into().ok()?),
    })
}

#[derive(Debug, Clone)]
pub struct SessionExitedPayload {
    pub session_id: u32,
    pub exit_code: i32,
}

pub fn encode_session_exited(p: &SessionExitedPayload) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8);
    buf.extend_from_slice(&p.session_id.to_be_bytes());
    buf.extend_from_slice(&p.exit_code.to_be_bytes());
    buf
}

pub fn decode_session_exited(data: &[u8]) -> Option<SessionExitedPayload> {
    if data.len() < 8 {
        return None;
    }
    Some(SessionExitedPayload {
        session_id: u32::from_be_bytes(data[0..4].try_into().ok()?),
        exit_code: i32::from_be_bytes(data[4..8].try_into().ok()?),
    })
}

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub session_id: u32,
    pub name: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub closed_at: Option<String>,
    pub has_note: bool,
    pub has_tags: bool,
    pub is_pinned: bool,
    pub is_locked: bool,
    pub idle: String,
    pub foreground_pid: Option<u32>,
    pub foreground_name: String,
    pub foreground_cmd: String,
}

#[derive(Debug, Clone)]
pub struct ListSessionsRespPayload {
    pub sessions: Vec<SessionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessTreeNode {
    pub pid: u32,
    pub parent_pid: u32,
    pub depth: u8,
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessTreeRespPayload {
    pub nodes: Vec<ProcessTreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessStatsRespPayload {
    pub pid: Option<u32>,
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub rss_kib: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
}

pub fn encode_process_stats_resp(payload: &ProcessStatsRespPayload) -> Vec<u8> {
    let mut buf = Vec::with_capacity(45);
    buf.push(u8::from(payload.pid.is_some()));
    buf.extend_from_slice(&payload.pid.unwrap_or_default().to_be_bytes());
    buf.extend_from_slice(&payload.user_ticks.to_be_bytes());
    buf.extend_from_slice(&payload.system_ticks.to_be_bytes());
    buf.extend_from_slice(&payload.rss_kib.to_be_bytes());
    buf.extend_from_slice(&payload.read_bytes.to_be_bytes());
    buf.extend_from_slice(&payload.write_bytes.to_be_bytes());
    buf
}

pub fn decode_process_stats_resp(data: &[u8]) -> Option<ProcessStatsRespPayload> {
    if data.len() != 45 {
        return None;
    }
    Some(ProcessStatsRespPayload {
        pid: (data[0] != 0).then_some(u32::from_be_bytes(data[1..5].try_into().ok()?)),
        user_ticks: u64::from_be_bytes(data[5..13].try_into().ok()?),
        system_ticks: u64::from_be_bytes(data[13..21].try_into().ok()?),
        rss_kib: u64::from_be_bytes(data[21..29].try_into().ok()?),
        read_bytes: u64::from_be_bytes(data[29..37].try_into().ok()?),
        write_bytes: u64::from_be_bytes(data[37..45].try_into().ok()?),
    })
}

pub fn encode_process_tree_resp(payload: &ProcessTreeRespPayload) -> Vec<u8> {
    let mut buf = Vec::new();
    let nodes = &payload.nodes[..payload.nodes.len().min(u16::MAX as usize)];
    buf.extend_from_slice(&(nodes.len() as u16).to_be_bytes());
    for node in nodes {
        buf.extend_from_slice(&node.pid.to_be_bytes());
        buf.extend_from_slice(&node.parent_pid.to_be_bytes());
        buf.push(node.depth);
        encode_str16(&mut buf, &node.name);
        encode_str16(&mut buf, &node.command);
    }
    buf
}

pub fn decode_process_tree_resp(data: &[u8]) -> Option<ProcessTreeRespPayload> {
    if data.len() < 2 {
        return None;
    }
    let count = u16::from_be_bytes(data[..2].try_into().ok()?) as usize;
    let mut offset = 2;
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        if offset + 9 > data.len() {
            return None;
        }
        let pid = u32::from_be_bytes(data[offset..offset + 4].try_into().ok()?);
        let parent_pid = u32::from_be_bytes(data[offset + 4..offset + 8].try_into().ok()?);
        let depth = data[offset + 8];
        offset += 9;
        let (name, next) = decode_str16(data, offset)?;
        offset = next;
        let (command, next) = decode_str16(data, offset)?;
        offset = next;
        nodes.push(ProcessTreeNode {
            pid,
            parent_pid,
            depth,
            name,
            command,
        });
    }
    Some(ProcessTreeRespPayload { nodes })
}

pub fn encode_list_sessions_resp(p: &ListSessionsRespPayload) -> Vec<u8> {
    let mut buf = Vec::new();
    let count = p.sessions.len().min(u16::MAX as usize) as u16;
    buf.extend_from_slice(&count.to_be_bytes());
    for entry in &p.sessions {
        buf.extend_from_slice(&entry.session_id.to_be_bytes());
        encode_str16(&mut buf, &entry.name);
        encode_str16(&mut buf, &entry.status);
        let mut flags: u8 = 0;
        if entry.exit_code.is_some() {
            flags |= 0x01;
        }
        if entry.closed_at.is_some() {
            flags |= 0x02;
        }
        if entry.has_note {
            flags |= 0x04;
        }
        if entry.has_tags {
            flags |= 0x08;
        }
        if entry.is_pinned {
            flags |= 0x10;
        }
        if entry.is_locked {
            flags |= 0x40;
        }
        if !entry.idle.is_empty() {
            flags |= 0x20;
        }
        if entry.foreground_pid.is_some() {
            flags |= 0x80;
        }
        buf.push(flags);
        if let Some(code) = entry.exit_code {
            buf.extend_from_slice(&code.to_be_bytes());
        }
        if let Some(ref closed) = entry.closed_at {
            encode_str16(&mut buf, closed);
        }
        if !entry.idle.is_empty() {
            encode_str16(&mut buf, &entry.idle);
        }
        if let Some(pid) = entry.foreground_pid {
            buf.extend_from_slice(&pid.to_be_bytes());
            encode_str16(&mut buf, &entry.foreground_name);
            encode_str16(&mut buf, &entry.foreground_cmd);
        }
    }
    buf
}

pub fn decode_list_sessions_resp(data: &[u8]) -> Option<ListSessionsRespPayload> {
    if data.len() < 2 {
        return None;
    }
    let count = u16::from_be_bytes(data[0..2].try_into().ok()?) as usize;
    let mut offset = 2;
    let mut sessions = Vec::with_capacity(count);
    for _ in 0..count {
        if offset + 4 > data.len() {
            return None;
        }
        let session_id = u32::from_be_bytes(data[offset..offset + 4].try_into().ok()?);
        offset += 4;
        let (name, new_offset) = decode_str16(data, offset)?;
        offset = new_offset;
        let (status, new_offset) = decode_str16(data, offset)?;
        offset = new_offset;
        if offset >= data.len() {
            return None;
        }
        let flags = data[offset];
        offset += 1;
        let exit_code = if flags & 0x01 != 0 {
            if offset + 4 > data.len() {
                return None;
            }
            let code = i32::from_be_bytes(data[offset..offset + 4].try_into().ok()?);
            offset += 4;
            Some(code)
        } else {
            None
        };
        let closed_at = if flags & 0x02 != 0 {
            let (s, new_offset) = decode_str16(data, offset)?;
            offset = new_offset;
            Some(s)
        } else {
            None
        };
        let has_note = flags & 0x04 != 0;
        let has_tags = flags & 0x08 != 0;
        let is_pinned = flags & 0x10 != 0;
        let is_locked = flags & 0x40 != 0;
        let idle = if flags & 0x20 != 0 {
            let (s, new_offset) = decode_str16(data, offset)?;
            offset = new_offset;
            s
        } else {
            String::new()
        };
        let (foreground_pid, foreground_name, foreground_cmd) = if flags & 0x80 != 0 {
            if offset + 4 > data.len() {
                return None;
            }
            let pid = u32::from_be_bytes(data[offset..offset + 4].try_into().ok()?);
            offset += 4;
            let (name, new_offset) = decode_str16(data, offset)?;
            offset = new_offset;
            let (cmd, new_offset) = decode_str16(data, offset)?;
            offset = new_offset;
            (Some(pid), name, cmd)
        } else {
            (None, String::new(), String::new())
        };
        sessions.push(SessionEntry {
            session_id,
            name,
            status,
            exit_code,
            closed_at,
            has_note,
            has_tags,
            is_pinned,
            is_locked,
            idle,
            foreground_pid,
            foreground_name,
            foreground_cmd,
        });
    }
    Some(ListSessionsRespPayload { sessions })
}

fn encode_str16(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(u16::MAX as usize) as u16;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&bytes[..len as usize]);
}

fn decode_str16(data: &[u8], offset: usize) -> Option<(String, usize)> {
    if offset + 2 > data.len() {
        return None;
    }
    let len = u16::from_be_bytes(data[offset..offset + 2].try_into().ok()?) as usize;
    if offset + 2 + len > data.len() {
        return None;
    }
    let s = String::from_utf8_lossy(&data[offset + 2..offset + 2 + len]).to_string();
    Some((s, offset + 2 + len))
}

// ── Non-blocking frame reading from a buffer ──

#[derive(Debug)]
pub struct FrameAccumulator {
    buf: Vec<u8>,
}

impl FrameAccumulator {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn try_read(&mut self) -> Result<Option<Frame>> {
        if self.buf.len() < HEADER_SIZE {
            return Ok(None);
        }
        let payload_len = u32::from_be_bytes(self.buf[0..4].try_into().unwrap()) as usize;
        let total = HEADER_SIZE + payload_len;
        if self.buf.len() < total {
            return Ok(None);
        }
        let raw_type = u16::from_be_bytes(self.buf[4..6].try_into().unwrap());
        let flags = u16::from_be_bytes(self.buf[6..8].try_into().unwrap());
        let request_id = u32::from_be_bytes(self.buf[8..12].try_into().unwrap());

        let msg_type = MessageType::from_u16(raw_type).ok_or_else(|| {
            PersistError::invalid_argument(format!("unknown message type: 0x{raw_type:04X}"))
        })?;

        let limit = if msg_type.is_io_message() {
            MAX_IO_FRAME
        } else {
            MAX_CONTROL_FRAME
        };
        if payload_len > limit {
            return Err(PersistError::invalid_argument(format!(
                "frame payload too large: {payload_len} > {limit}"
            )));
        }

        let payload = if payload_len > 0 {
            self.buf[HEADER_SIZE..total].to_vec()
        } else {
            Vec::new()
        };

        self.buf.drain(..total);
        Ok(Some(Frame {
            msg_type,
            flags,
            request_id,
            payload,
        }))
    }
}

impl Default for FrameAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

pub fn set_stream_timeout(
    stream: &UnixStream,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
) -> Result<()> {
    stream
        .set_read_timeout(read_timeout)
        .map_err(|source| PersistError::Io {
            operation: "set read timeout",
            source,
        })?;
    stream
        .set_write_timeout(write_timeout)
        .map_err(|source| PersistError::Io {
            operation: "set write timeout",
            source,
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    pub const CURRENT: Self = Self { major: 0, minor: 1 };

    pub fn is_compatible(&self, other: &Self) -> bool {
        self.major == other.major
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (UnixStream, UnixStream) {
        UnixStream::pair().expect("create pair")
    }

    #[test]
    fn round_trip_frame() {
        let (mut a, mut b) = pair();
        let frame = Frame {
            msg_type: MessageType::Hello,
            flags: 0,
            request_id: 42,
            payload: vec![1, 2, 3, 4],
        };

        write_frame(&mut a, &frame).expect("write");
        let received = read_frame(&mut b).expect("read");

        assert_eq!(received.msg_type, MessageType::Hello);
        assert_eq!(received.flags, 0);
        assert_eq!(received.request_id, 42);
        assert_eq!(received.payload, vec![1, 2, 3, 4]);
    }

    #[test]
    fn round_trip_hello() {
        let (mut a, mut b) = pair();
        let hello = HelloPayload {
            protocol_major: 0,
            protocol_minor: 1,
            uid: 1000,
            pid: 12345,
        };
        let payload = encode_hello(&hello);

        write_frame(
            &mut a,
            &Frame {
                msg_type: MessageType::Hello,
                flags: 0,
                request_id: 0,
                payload,
            },
        )
        .expect("write hello");

        let frame = read_frame(&mut b).expect("read hello");
        assert_eq!(frame.msg_type, MessageType::Hello);

        let decoded = decode_hello(&frame.payload).expect("decode hello");
        assert_eq!(decoded.protocol_major, 0);
        assert_eq!(decoded.protocol_minor, 1);
        assert_eq!(decoded.uid, 1000);
        assert_eq!(decoded.pid, 12345);
    }

    #[test]
    fn round_trip_hello_ack() {
        let (mut a, mut b) = pair();
        let ack = HelloAckPayload {
            protocol_major: 0,
            protocol_minor: 1,
            pid: 999,
            status: HelloStatus::Accepted,
        };
        let payload = encode_hello_ack(&ack);

        write_frame(
            &mut a,
            &Frame {
                msg_type: MessageType::HelloAck,
                flags: 0,
                request_id: 0,
                payload,
            },
        )
        .expect("write ack");

        let frame = read_frame(&mut b).expect("read ack");
        assert_eq!(frame.msg_type, MessageType::HelloAck);

        let decoded = decode_hello_ack(&frame.payload).expect("decode ack");
        assert_eq!(decoded.protocol_major, 0);
        assert_eq!(decoded.protocol_minor, 1);
        assert_eq!(decoded.pid, 999);
        assert_eq!(decoded.status, HelloStatus::Accepted);
    }

    #[test]
    fn empty_payload_frame() {
        let (mut a, mut b) = pair();
        write_frame(
            &mut a,
            &Frame {
                msg_type: MessageType::Ping,
                flags: 0,
                request_id: 0,
                payload: vec![],
            },
        )
        .expect("write ping");

        let frame = read_frame(&mut b).expect("read");
        assert_eq!(frame.msg_type, MessageType::Ping);
        assert!(frame.payload.is_empty());
    }

    #[test]
    fn decode_hello_rejects_short_payload() {
        assert!(decode_hello(&[0u8; 4]).is_none());
    }

    #[test]
    fn decode_hello_ack_rejects_short_payload() {
        assert!(decode_hello_ack(&[0u8; 4]).is_none());
    }

    #[test]
    fn hello_status_mapping() {
        let ack = HelloAckPayload {
            protocol_major: 0,
            protocol_minor: 1,
            pid: 0,
            status: HelloStatus::VersionMismatch,
        };
        let payload = encode_hello_ack(&ack);
        let decoded = decode_hello_ack(&payload).expect("decode");
        assert_eq!(decoded.status, HelloStatus::VersionMismatch);

        let ack2 = HelloAckPayload {
            status: HelloStatus::PermissionDenied,
            ..ack
        };
        let payload2 = encode_hello_ack(&ack2);
        let decoded2 = decode_hello_ack(&payload2).expect("decode");
        assert_eq!(decoded2.status, HelloStatus::PermissionDenied);
    }

    #[test]
    fn protocol_version_compatibility() {
        let v1 = ProtocolVersion { major: 0, minor: 1 };
        let v2 = ProtocolVersion { major: 0, minor: 2 };
        let v3 = ProtocolVersion { major: 1, minor: 0 };

        assert!(v1.is_compatible(&v2));
        assert!(!v1.is_compatible(&v3));
    }

    #[test]
    fn process_tree_round_trip() {
        let payload = ProcessTreeRespPayload {
            nodes: vec![ProcessTreeNode {
                pid: 42,
                parent_pid: 1,
                depth: 2,
                name: "make".into(),
                command: "make -j8".into(),
            }],
        };
        assert_eq!(
            decode_process_tree_resp(&encode_process_tree_resp(&payload)),
            Some(payload)
        );
    }

    #[test]
    fn observability_message_types_round_trip() {
        assert_eq!(MessageType::from_u16(0x0036), Some(MessageType::Metrics));
        assert_eq!(
            MessageType::from_u16(0x0037),
            Some(MessageType::MetricsResp)
        );
        assert_eq!(
            MessageType::from_u16(0x0038),
            Some(MessageType::DashboardSummary)
        );
        assert_eq!(
            MessageType::from_u16(0x0039),
            Some(MessageType::DashboardSummaryResp)
        );
        assert_eq!(
            MessageType::from_u16(0x003A),
            Some(MessageType::DashboardTrend)
        );
        assert_eq!(
            MessageType::from_u16(0x003B),
            Some(MessageType::DashboardTrendResp)
        );
    }

    #[test]
    fn unknown_message_type_is_rejected() {
        let (mut a, mut b) = pair();
        let buf = [
            &0u32.to_be_bytes()[..],
            &0xFFFFu16.to_be_bytes()[..],
            &0u16.to_be_bytes()[..],
            &0u32.to_be_bytes()[..],
        ]
        .concat();
        a.write_all(&buf).expect("write");

        let result = read_frame(&mut b);
        assert!(result.is_err());
    }

    #[test]
    fn lock_payload_round_trip_and_invalid_length() {
        let payload = LockPayload {
            session_id: 42,
            locked: true,
        };
        let encoded = encode_lock(&payload);
        let decoded = decode_lock(&encoded).expect("decode lock payload");
        assert_eq!(decoded.session_id, 42);
        assert!(decoded.locked);
        assert!(decode_lock(&encoded[..4]).is_none());
    }

    #[test]
    fn session_entry_round_trip_preserves_locked_flag() {
        let payload = ListSessionsRespPayload {
            sessions: vec![SessionEntry {
                session_id: 42,
                name: "locked".into(),
                status: "running".into(),
                exit_code: None,
                closed_at: None,
                has_note: false,
                has_tags: false,
                is_pinned: false,
                is_locked: true,
                idle: String::new(),
                foreground_pid: Some(42),
                foreground_name: "sh".into(),
                foreground_cmd: "/bin/sh -i".into(),
            }],
        };
        let decoded = decode_list_sessions_resp(&encode_list_sessions_resp(&payload))
            .expect("decode session list");
        assert!(decoded.sessions[0].is_locked);
        assert_eq!(decoded.sessions[0].foreground_pid, Some(42));
        assert_eq!(decoded.sessions[0].foreground_name, "sh");
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let (mut a, mut b) = pair();
        let buf = [
            &(MAX_CONTROL_FRAME as u32 + 1).to_be_bytes()[..],
            &(MessageType::Hello as u16).to_be_bytes()[..],
            &0u16.to_be_bytes()[..],
            &0u32.to_be_bytes()[..],
        ]
        .concat();
        a.write_all(&buf).expect("write");

        let result = read_frame(&mut b);
        assert!(result.is_err());
    }

    #[test]
    fn zero_length_frame() {
        let (mut a, mut b) = pair();
        let buf = [
            &0u32.to_be_bytes()[..],
            &(MessageType::Ping as u16).to_be_bytes()[..],
            &0u16.to_be_bytes()[..],
            &0u32.to_be_bytes()[..],
        ]
        .concat();
        a.write_all(&buf).expect("write");

        let frame = read_frame(&mut b).expect("read");
        assert_eq!(frame.msg_type, MessageType::Ping);
        assert_eq!(frame.payload.len(), 0);
    }

    #[test]
    fn writer_control_round_trip_and_rejects_invalid_length() {
        let payload = WriterControlPayload { session_id: 42 };
        assert_eq!(
            decode_writer_control(&encode_writer_control(&payload)),
            Some(payload)
        );
        assert_eq!(decode_writer_control(&[0; 3]), None);
        assert_eq!(
            MessageType::from_u16(0x002B),
            Some(MessageType::WriteRequest)
        );
        assert_eq!(
            MessageType::from_u16(0x002C),
            Some(MessageType::WriteGranted)
        );
        assert_eq!(
            MessageType::from_u16(0x002D),
            Some(MessageType::WriteRevoked)
        );
    }
}
