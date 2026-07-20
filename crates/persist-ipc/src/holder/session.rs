use std::path::Path;

use persist_core::shell_state::ShellLaunchEnvironment;

use super::wire::{
    put_optional_string, put_string, put_string_allow_empty, put_u16, put_u32, put_u8, Reader,
};
use super::{
    AttachRequest, CreateSessionRequest, HolderAttachMode, HolderProtocolError,
    MAX_HOLDER_ARGUMENT, MAX_HOLDER_ARGUMENTS, MAX_HOLDER_ENV_NAME, MAX_HOLDER_ENV_VALUE,
    MAX_HOLDER_ENV_VARS, MAX_HOLDER_PATH, MAX_HOLDER_RING_BUFFER,
};

pub fn encode_create_request(
    request: &CreateSessionRequest,
) -> Result<Vec<u8>, HolderProtocolError> {
    encode_create(request, false)
}

pub fn encode_create_request_v2(
    request: &CreateSessionRequest,
) -> Result<Vec<u8>, HolderProtocolError> {
    encode_create(request, true)
}

fn encode_create(
    request: &CreateSessionRequest,
    structured_environment: bool,
) -> Result<Vec<u8>, HolderProtocolError> {
    validate_create(request)?;
    let mut output = Vec::new();
    put_u32(&mut output, request.session_id);
    put_u32(&mut output, request.ring_buffer_size);
    put_string(&mut output, &request.shell, MAX_HOLDER_PATH)?;
    put_u16(&mut output, request.arguments.len() as u16);
    for argument in &request.arguments {
        put_string_allow_empty(&mut output, argument, MAX_HOLDER_ARGUMENT)?;
    }
    put_optional_string(&mut output, request.cwd.as_deref(), MAX_HOLDER_PATH)?;
    if structured_environment {
        put_environment_pairs(&mut output, request.launch_environment.saved_set())?;
        put_environment_names(&mut output, request.launch_environment.saved_unset())?;
        put_environment_pairs(&mut output, request.launch_environment.connection())?;
        put_environment_pairs(&mut output, request.launch_environment.private())?;
    } else {
        let legacy = request.launch_environment.legacy_set_environment();
        put_environment_pairs(&mut output, &legacy)?;
    }
    put_optional_string(
        &mut output,
        request.history_file.as_deref(),
        MAX_HOLDER_PATH,
    )?;
    put_optional_string(&mut output, request.log_path.as_deref(), MAX_HOLDER_PATH)?;
    put_string(&mut output, &request.state_file, MAX_HOLDER_PATH)?;
    output.extend_from_slice(&request.state_incarnation);
    Ok(output)
}

fn put_environment_pairs(
    output: &mut Vec<u8>,
    environment: &[(String, String)],
) -> Result<(), HolderProtocolError> {
    put_u16(output, environment.len() as u16);
    for (name, value) in environment {
        put_string(output, name, MAX_HOLDER_ENV_NAME)?;
        put_string_allow_empty(output, value, MAX_HOLDER_ENV_VALUE)?;
    }
    Ok(())
}

fn put_environment_names(
    output: &mut Vec<u8>,
    names: &[String],
) -> Result<(), HolderProtocolError> {
    put_u16(output, names.len() as u16);
    for name in names {
        put_string(output, name, MAX_HOLDER_ENV_NAME)?;
    }
    Ok(())
}

pub fn decode_create_request(payload: &[u8]) -> Result<CreateSessionRequest, HolderProtocolError> {
    decode_create(payload, false)
}

pub fn decode_create_request_v2(
    payload: &[u8],
) -> Result<CreateSessionRequest, HolderProtocolError> {
    decode_create(payload, true)
}

fn decode_create(
    payload: &[u8],
    structured_environment: bool,
) -> Result<CreateSessionRequest, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let session_id = reader.u32()?;
    let ring_buffer_size = reader.u32()?;
    let shell = reader.string(MAX_HOLDER_PATH)?;
    let argument_count = reader.u16()? as usize;
    if argument_count > MAX_HOLDER_ARGUMENTS {
        return Err(HolderProtocolError::PayloadTooLarge);
    }
    let mut arguments = Vec::with_capacity(argument_count);
    for _ in 0..argument_count {
        arguments.push(reader.string(MAX_HOLDER_ARGUMENT)?);
    }
    let cwd = reader.optional_string(MAX_HOLDER_PATH)?;
    let launch_environment = if structured_environment {
        let saved_set = read_environment_pairs(&mut reader)?;
        let saved_unset = read_environment_names(&mut reader)?;
        let connection = read_environment_pairs(&mut reader)?;
        let private = read_environment_pairs(&mut reader)?;
        if saved_set.len() + saved_unset.len() + connection.len() + private.len()
            > MAX_HOLDER_ENV_VARS
        {
            return Err(HolderProtocolError::PayloadTooLarge);
        }
        ShellLaunchEnvironment::new(saved_set, saved_unset, connection, private)
            .map_err(|_| HolderProtocolError::InvalidField)?
    } else {
        ShellLaunchEnvironment::legacy(read_environment_pairs(&mut reader)?)
            .map_err(|_| HolderProtocolError::InvalidField)?
    };
    let request = CreateSessionRequest {
        session_id,
        shell,
        arguments,
        cwd,
        launch_environment,
        history_file: reader.optional_string(MAX_HOLDER_PATH)?,
        ring_buffer_size,
        log_path: reader.optional_string(MAX_HOLDER_PATH)?,
        state_file: reader.string(MAX_HOLDER_PATH)?,
        state_incarnation: reader.fixed::<16>()?,
    };
    reader.finish()?;
    validate_create(&request)?;
    Ok(request)
}

fn read_environment_pairs(
    reader: &mut Reader<'_>,
) -> Result<Vec<(String, String)>, HolderProtocolError> {
    let count = reader.u16()? as usize;
    if count > MAX_HOLDER_ENV_VARS {
        return Err(HolderProtocolError::PayloadTooLarge);
    }
    let mut environment = Vec::with_capacity(count);
    for _ in 0..count {
        environment.push((
            reader.string(MAX_HOLDER_ENV_NAME)?,
            reader.string(MAX_HOLDER_ENV_VALUE)?,
        ));
    }
    Ok(environment)
}

fn read_environment_names(reader: &mut Reader<'_>) -> Result<Vec<String>, HolderProtocolError> {
    let count = reader.u16()? as usize;
    if count > MAX_HOLDER_ENV_VARS {
        return Err(HolderProtocolError::PayloadTooLarge);
    }
    (0..count)
        .map(|_| reader.string(MAX_HOLDER_ENV_NAME))
        .collect()
}

pub fn encode_attach_request(request: &AttachRequest) -> Vec<u8> {
    if request.session_id == 0 || request.replay_bytes > MAX_HOLDER_RING_BUFFER {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(9);
    put_u32(&mut output, request.session_id);
    put_u8(
        &mut output,
        match request.mode {
            HolderAttachMode::ReadWrite => 0,
            HolderAttachMode::ReadOnly => 1,
        },
    );
    put_u32(&mut output, request.replay_bytes);
    output
}

pub fn decode_attach_request(payload: &[u8]) -> Result<AttachRequest, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let session_id = reader.u32()?;
    let mode = match reader.u8()? {
        0 => HolderAttachMode::ReadWrite,
        1 => HolderAttachMode::ReadOnly,
        _ => return Err(HolderProtocolError::InvalidField),
    };
    let replay_bytes = reader.u32()?;
    reader.finish()?;
    if session_id == 0 || replay_bytes > MAX_HOLDER_RING_BUFFER {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(AttachRequest {
        session_id,
        mode,
        replay_bytes,
    })
}

fn validate_create(request: &CreateSessionRequest) -> Result<(), HolderProtocolError> {
    if request.shell.len() > MAX_HOLDER_PATH
        || request.arguments.len() > MAX_HOLDER_ARGUMENTS
        || request
            .arguments
            .iter()
            .any(|argument| argument.len() > MAX_HOLDER_ARGUMENT)
        || request.launch_environment.entry_count() > MAX_HOLDER_ENV_VARS
        || request
            .cwd
            .as_ref()
            .is_some_and(|value| value.len() > MAX_HOLDER_PATH)
        || request
            .history_file
            .as_ref()
            .is_some_and(|value| value.len() > MAX_HOLDER_PATH)
        || request
            .log_path
            .as_ref()
            .is_some_and(|value| value.len() > MAX_HOLDER_PATH)
        || request.state_file.len() > MAX_HOLDER_PATH
    {
        return Err(HolderProtocolError::PayloadTooLarge);
    }
    if request.session_id == 0
        || request.ring_buffer_size == 0
        || request.ring_buffer_size > MAX_HOLDER_RING_BUFFER
        || !is_absolute(&request.shell)
        || !optional_absolute(request.cwd.as_deref())
        || !optional_absolute(request.history_file.as_deref())
        || !optional_absolute(request.log_path.as_deref())
        || !is_absolute(&request.state_file)
        || request.state_incarnation == [0; 16]
    {
        return Err(HolderProtocolError::InvalidField);
    }
    if request
        .arguments
        .iter()
        .any(|argument| argument.as_bytes().contains(&0))
    {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(())
}

fn is_absolute(value: &str) -> bool {
    !value.is_empty() && !value.as_bytes().contains(&0) && Path::new(value).is_absolute()
}

fn optional_absolute(value: Option<&str>) -> bool {
    value.map(is_absolute).unwrap_or(true)
}
