use super::wire::{put_u32, put_u8, Reader};
use super::{
    ControlHello, ControlHelloAck, DataHello, DataHelloAck, HelloStatus, HolderProtocolError,
};

const CONTROL_HELLO_SIZE: usize = 24;
const CONTROL_ACK_SIZE: usize = 37;
const DATA_HELLO_SIZE: usize = 36;
const DATA_ACK_SIZE: usize = 33;

pub fn encode_control_hello(hello: &ControlHello) -> Vec<u8> {
    let mut output = Vec::with_capacity(CONTROL_HELLO_SIZE);
    put_u32(&mut output, hello.uid);
    put_u32(&mut output, hello.daemon_pid);
    output.extend_from_slice(&hello.nonce);
    output
}

pub fn decode_control_hello(payload: &[u8]) -> Result<ControlHello, HolderProtocolError> {
    if payload.len() != CONTROL_HELLO_SIZE {
        return Err(length_error(payload.len(), CONTROL_HELLO_SIZE));
    }
    let mut reader = Reader::new(payload);
    let hello = ControlHello {
        uid: reader.u32()?,
        daemon_pid: reader.u32()?,
        nonce: reader.fixed()?,
    };
    reader.finish()?;
    if hello.daemon_pid == 0 || hello.nonce == [0; 16] {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(hello)
}

pub fn encode_control_hello_ack(ack: &ControlHelloAck) -> Vec<u8> {
    let mut output = Vec::with_capacity(CONTROL_ACK_SIZE);
    put_u32(&mut output, ack.holder_pid);
    output.extend_from_slice(&ack.instance_id);
    output.extend_from_slice(&ack.nonce);
    put_u8(&mut output, encode_status(ack.status));
    output
}

pub fn decode_control_hello_ack(payload: &[u8]) -> Result<ControlHelloAck, HolderProtocolError> {
    if payload.len() != CONTROL_ACK_SIZE {
        return Err(length_error(payload.len(), CONTROL_ACK_SIZE));
    }
    let mut reader = Reader::new(payload);
    let ack = ControlHelloAck {
        holder_pid: reader.u32()?,
        instance_id: reader.fixed()?,
        nonce: reader.fixed()?,
        status: decode_status(reader.u8()?)?,
    };
    reader.finish()?;
    if ack.holder_pid == 0 || ack.instance_id == [0; 16] || ack.nonce == [0; 16] {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(ack)
}

pub fn encode_data_hello(hello: &DataHello) -> Vec<u8> {
    let mut output = Vec::with_capacity(DATA_HELLO_SIZE);
    put_u32(&mut output, hello.daemon_pid);
    output.extend_from_slice(&hello.instance_id);
    output.extend_from_slice(&hello.nonce);
    output
}

pub fn decode_data_hello(payload: &[u8]) -> Result<DataHello, HolderProtocolError> {
    if payload.len() != DATA_HELLO_SIZE {
        return Err(length_error(payload.len(), DATA_HELLO_SIZE));
    }
    let mut reader = Reader::new(payload);
    let hello = DataHello {
        daemon_pid: reader.u32()?,
        instance_id: reader.fixed()?,
        nonce: reader.fixed()?,
    };
    reader.finish()?;
    if hello.daemon_pid == 0 || hello.instance_id == [0; 16] || hello.nonce == [0; 16] {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(hello)
}

pub fn encode_data_hello_ack(ack: &DataHelloAck) -> Vec<u8> {
    let mut output = Vec::with_capacity(DATA_ACK_SIZE);
    output.extend_from_slice(&ack.instance_id);
    output.extend_from_slice(&ack.nonce);
    put_u8(&mut output, encode_status(ack.status));
    output
}

pub fn decode_data_hello_ack(payload: &[u8]) -> Result<DataHelloAck, HolderProtocolError> {
    if payload.len() != DATA_ACK_SIZE {
        return Err(length_error(payload.len(), DATA_ACK_SIZE));
    }
    let mut reader = Reader::new(payload);
    let ack = DataHelloAck {
        instance_id: reader.fixed()?,
        nonce: reader.fixed()?,
        status: decode_status(reader.u8()?)?,
    };
    reader.finish()?;
    if ack.instance_id == [0; 16] || ack.nonce == [0; 16] {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(ack)
}

fn encode_status(status: HelloStatus) -> u8 {
    match status {
        HelloStatus::Accepted => 0,
        HelloStatus::VersionMismatch => 1,
        HelloStatus::PermissionDenied => 2,
        HelloStatus::Busy => 3,
    }
}

fn decode_status(value: u8) -> Result<HelloStatus, HolderProtocolError> {
    match value {
        0 => Ok(HelloStatus::Accepted),
        1 => Ok(HelloStatus::VersionMismatch),
        2 => Ok(HelloStatus::PermissionDenied),
        3 => Ok(HelloStatus::Busy),
        _ => Err(HolderProtocolError::InvalidField),
    }
}

fn length_error(actual: usize, expected: usize) -> HolderProtocolError {
    if actual < expected {
        HolderProtocolError::Truncated
    } else {
        HolderProtocolError::Trailing
    }
}
