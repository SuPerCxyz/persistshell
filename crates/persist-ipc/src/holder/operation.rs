use std::path::Path;

use persist_core::shell_state::{
    decode_environment_snapshot, encode_environment_snapshot, EnvironmentSnapshot,
    MAX_SHELL_ENVIRONMENT_BYTES,
};

use super::wire::{
    put_i32, put_optional_bytes_u32, put_optional_string, put_string_allow_empty, put_u16, put_u32,
    put_u64, put_u8, Reader,
};
use super::{HolderProtocolError, MAX_HOLDER_ERROR_MESSAGE, MAX_HOLDER_PATH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationRequest {
    pub session_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStatus {
    Ok,
    NotFound,
    Conflict,
    Rejected,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationResponse {
    pub session_id: u32,
    pub status: OperationStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionExitedEvent {
    pub session_id: u32,
    pub exit_code: i32,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionExitedEventV2 {
    pub session_id: u32,
    pub exit_code: i32,
    pub cwd: Option<String>,
    pub environment: Option<EnvironmentSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitContextResponse {
    pub session_id: u32,
    pub status: OperationStatus,
    pub exit_code: Option<i32>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitContextResponseV2 {
    pub session_id: u32,
    pub status: OperationStatus,
    pub exit_code: Option<i32>,
    pub cwd: Option<String>,
    pub environment: Option<EnvironmentSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeRequest {
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalRequest {
    pub signal: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogDegradedEvent {
    pub session_id: u32,
    pub dropped_bytes: u64,
}

pub fn encode_operation_request(request: &OperationRequest) -> Vec<u8> {
    if request.session_id == 0 {
        return Vec::new();
    }
    request.session_id.to_be_bytes().to_vec()
}

pub fn decode_operation_request(payload: &[u8]) -> Result<OperationRequest, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let request = OperationRequest {
        session_id: reader.u32()?,
    };
    reader.finish()?;
    if request.session_id == 0 {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(request)
}

pub fn encode_operation_response(
    response: &OperationResponse,
) -> Result<Vec<u8>, HolderProtocolError> {
    if response.session_id == 0 || response.message.len() > MAX_HOLDER_ERROR_MESSAGE {
        return Err(if response.message.len() > MAX_HOLDER_ERROR_MESSAGE {
            HolderProtocolError::PayloadTooLarge
        } else {
            HolderProtocolError::InvalidField
        });
    }
    let mut output = Vec::new();
    put_u32(&mut output, response.session_id);
    put_u8(&mut output, encode_status(response.status));
    put_string_allow_empty(&mut output, &response.message, MAX_HOLDER_ERROR_MESSAGE)?;
    Ok(output)
}

pub fn decode_operation_response(payload: &[u8]) -> Result<OperationResponse, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let response = OperationResponse {
        session_id: reader.u32()?,
        status: decode_status(reader.u8()?)?,
        message: reader.string(MAX_HOLDER_ERROR_MESSAGE)?,
    };
    reader.finish()?;
    if response.session_id == 0 {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(response)
}

pub fn encode_session_exited_event(
    event: &SessionExitedEvent,
) -> Result<Vec<u8>, HolderProtocolError> {
    if event.session_id == 0 {
        return Err(HolderProtocolError::InvalidField);
    }
    validate_cwd(event.cwd.as_deref())?;
    let mut output = Vec::with_capacity(9 + event.cwd.as_ref().map_or(0, String::len));
    put_u32(&mut output, event.session_id);
    put_i32(&mut output, event.exit_code);
    put_optional_string(&mut output, event.cwd.as_deref(), MAX_HOLDER_PATH)?;
    Ok(output)
}

pub fn decode_session_exited_event(
    payload: &[u8],
) -> Result<SessionExitedEvent, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let event = SessionExitedEvent {
        session_id: reader.u32()?,
        exit_code: reader.i32()?,
        cwd: reader.optional_string(MAX_HOLDER_PATH)?,
    };
    reader.finish()?;
    if event.session_id == 0 {
        return Err(HolderProtocolError::InvalidField);
    }
    validate_cwd(event.cwd.as_deref())?;
    Ok(event)
}

pub fn encode_session_exited_event_v2(
    event: &SessionExitedEventV2,
) -> Result<Vec<u8>, HolderProtocolError> {
    let legacy = SessionExitedEvent {
        session_id: event.session_id,
        exit_code: event.exit_code,
        cwd: event.cwd.clone(),
    };
    let mut output = encode_session_exited_event(&legacy)?;
    let environment = event
        .environment
        .as_ref()
        .map(encode_environment_snapshot)
        .transpose()
        .map_err(|_| HolderProtocolError::InvalidField)?;
    put_optional_bytes_u32(
        &mut output,
        environment.as_deref(),
        MAX_SHELL_ENVIRONMENT_BYTES,
    )?;
    Ok(output)
}

pub fn decode_session_exited_event_v2(
    payload: &[u8],
) -> Result<SessionExitedEventV2, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let event = SessionExitedEventV2 {
        session_id: reader.u32()?,
        exit_code: reader.i32()?,
        cwd: reader.optional_string(MAX_HOLDER_PATH)?,
        environment: reader
            .optional_bytes_u32(MAX_SHELL_ENVIRONMENT_BYTES)?
            .map(|encoded| decode_environment_snapshot(&encoded))
            .transpose()
            .map_err(|_| HolderProtocolError::InvalidField)?,
    };
    reader.finish()?;
    if event.session_id == 0 {
        return Err(HolderProtocolError::InvalidField);
    }
    validate_cwd(event.cwd.as_deref())?;
    Ok(event)
}

pub fn encode_exit_context_response(
    response: &ExitContextResponse,
) -> Result<Vec<u8>, HolderProtocolError> {
    validate_exit_context_response(response)?;
    let mut output = Vec::new();
    put_u32(&mut output, response.session_id);
    put_u8(&mut output, encode_status(response.status));
    put_u8(&mut output, u8::from(response.exit_code.is_some()));
    put_i32(&mut output, response.exit_code.unwrap_or_default());
    put_optional_string(&mut output, response.cwd.as_deref(), MAX_HOLDER_PATH)?;
    Ok(output)
}

pub fn decode_exit_context_response(
    payload: &[u8],
) -> Result<ExitContextResponse, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let session_id = reader.u32()?;
    let status = decode_status(reader.u8()?)?;
    let has_exit_code = match reader.u8()? {
        0 => false,
        1 => true,
        _ => return Err(HolderProtocolError::InvalidField),
    };
    let raw_exit_code = reader.i32()?;
    let response = ExitContextResponse {
        session_id,
        status,
        exit_code: has_exit_code.then_some(raw_exit_code),
        cwd: reader.optional_string(MAX_HOLDER_PATH)?,
    };
    reader.finish()?;
    validate_exit_context_response(&response)?;
    Ok(response)
}

pub fn encode_exit_context_response_v2(
    response: &ExitContextResponseV2,
) -> Result<Vec<u8>, HolderProtocolError> {
    validate_exit_context_response_v2(response)?;
    let legacy = ExitContextResponse {
        session_id: response.session_id,
        status: response.status,
        exit_code: response.exit_code,
        cwd: response.cwd.clone(),
    };
    let mut output = encode_exit_context_response(&legacy)?;
    let environment = response
        .environment
        .as_ref()
        .map(encode_environment_snapshot)
        .transpose()
        .map_err(|_| HolderProtocolError::InvalidField)?;
    put_optional_bytes_u32(
        &mut output,
        environment.as_deref(),
        MAX_SHELL_ENVIRONMENT_BYTES,
    )?;
    Ok(output)
}

pub fn decode_exit_context_response_v2(
    payload: &[u8],
) -> Result<ExitContextResponseV2, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let session_id = reader.u32()?;
    let status = decode_status(reader.u8()?)?;
    let has_exit_code = match reader.u8()? {
        0 => false,
        1 => true,
        _ => return Err(HolderProtocolError::InvalidField),
    };
    let raw_exit_code = reader.i32()?;
    let response = ExitContextResponseV2 {
        session_id,
        status,
        exit_code: has_exit_code.then_some(raw_exit_code),
        cwd: reader.optional_string(MAX_HOLDER_PATH)?,
        environment: reader
            .optional_bytes_u32(MAX_SHELL_ENVIRONMENT_BYTES)?
            .map(|encoded| decode_environment_snapshot(&encoded))
            .transpose()
            .map_err(|_| HolderProtocolError::InvalidField)?,
    };
    reader.finish()?;
    validate_exit_context_response_v2(&response)?;
    Ok(response)
}

fn validate_exit_context_response_v2(
    response: &ExitContextResponseV2,
) -> Result<(), HolderProtocolError> {
    validate_exit_context_response(&ExitContextResponse {
        session_id: response.session_id,
        status: response.status,
        exit_code: response.exit_code,
        cwd: response.cwd.clone(),
    })?;
    if response.status != OperationStatus::Ok && response.environment.is_some() {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(())
}

fn validate_exit_context_response(
    response: &ExitContextResponse,
) -> Result<(), HolderProtocolError> {
    if response.session_id == 0 {
        return Err(HolderProtocolError::InvalidField);
    }
    let valid_combination = match response.status {
        OperationStatus::Ok => response.exit_code.is_some(),
        _ => response.exit_code.is_none() && response.cwd.is_none(),
    };
    if !valid_combination {
        return Err(HolderProtocolError::InvalidField);
    }
    validate_cwd(response.cwd.as_deref())
}

fn validate_cwd(cwd: Option<&str>) -> Result<(), HolderProtocolError> {
    let Some(cwd) = cwd else {
        return Ok(());
    };
    if cwd.len() > MAX_HOLDER_PATH {
        return Err(HolderProtocolError::PayloadTooLarge);
    }
    if cwd.is_empty() || cwd.as_bytes().contains(&0) || !Path::new(cwd).is_absolute() {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(())
}

pub fn encode_resize_request(request: &ResizeRequest) -> Vec<u8> {
    if request.rows == 0 || request.cols == 0 {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(4);
    put_u16(&mut output, request.rows);
    put_u16(&mut output, request.cols);
    output
}

pub fn decode_resize_request(payload: &[u8]) -> Result<ResizeRequest, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let request = ResizeRequest {
        rows: reader.u16()?,
        cols: reader.u16()?,
    };
    reader.finish()?;
    if request.rows == 0 || request.cols == 0 {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(request)
}

pub fn encode_signal_request(request: &SignalRequest) -> Vec<u8> {
    if request.signal == 0 || request.signal > 64 {
        return Vec::new();
    }
    request.signal.to_be_bytes().to_vec()
}

pub fn decode_signal_request(payload: &[u8]) -> Result<SignalRequest, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let request = SignalRequest {
        signal: reader.u32()?,
    };
    reader.finish()?;
    if request.signal == 0 || request.signal > 64 {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(request)
}

pub fn encode_log_degraded(event: &LogDegradedEvent) -> Vec<u8> {
    if event.session_id == 0 || event.dropped_bytes == 0 {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(12);
    put_u32(&mut output, event.session_id);
    put_u64(&mut output, event.dropped_bytes);
    output
}

pub fn decode_log_degraded(payload: &[u8]) -> Result<LogDegradedEvent, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let event = LogDegradedEvent {
        session_id: reader.u32()?,
        dropped_bytes: reader.u64()?,
    };
    reader.finish()?;
    if event.session_id == 0 || event.dropped_bytes == 0 {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(event)
}

fn encode_status(status: OperationStatus) -> u8 {
    match status {
        OperationStatus::Ok => 0,
        OperationStatus::NotFound => 1,
        OperationStatus::Conflict => 2,
        OperationStatus::Rejected => 3,
        OperationStatus::Internal => 4,
    }
}

fn decode_status(value: u8) -> Result<OperationStatus, HolderProtocolError> {
    match value {
        0 => Ok(OperationStatus::Ok),
        1 => Ok(OperationStatus::NotFound),
        2 => Ok(OperationStatus::Conflict),
        3 => Ok(OperationStatus::Rejected),
        4 => Ok(OperationStatus::Internal),
        _ => Err(HolderProtocolError::InvalidField),
    }
}
