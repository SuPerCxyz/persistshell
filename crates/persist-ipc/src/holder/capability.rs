use super::wire::{put_u16, put_u64, Reader};
use super::{
    CapabilityRequest, CapabilityResponse, HolderProtocolError,
    HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT, HOLDER_PROTOCOL_BASELINE_MINOR, HOLDER_PROTOCOL_MINOR,
};

pub fn encode_capability_request(request: &CapabilityRequest) -> Vec<u8> {
    let mut output = Vec::with_capacity(34);
    output.extend_from_slice(&request.instance_id);
    output.extend_from_slice(&request.nonce);
    put_u16(&mut output, request.max_minor);
    output
}

pub fn decode_capability_request(payload: &[u8]) -> Result<CapabilityRequest, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let request = CapabilityRequest {
        instance_id: reader.fixed()?,
        nonce: reader.fixed()?,
        max_minor: reader.u16()?,
    };
    reader.finish()?;
    if request.instance_id == [0; 16] || request.nonce == [0; 16] || !valid_minor(request.max_minor)
    {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(request)
}

pub fn encode_capability_response(response: &CapabilityResponse) -> Vec<u8> {
    let mut output = Vec::with_capacity(42);
    output.extend_from_slice(&response.instance_id);
    output.extend_from_slice(&response.nonce);
    put_u16(&mut output, response.selected_minor);
    put_u64(&mut output, response.capabilities);
    output
}

pub fn decode_capability_response(
    payload: &[u8],
) -> Result<CapabilityResponse, HolderProtocolError> {
    let mut reader = Reader::new(payload);
    let response = CapabilityResponse {
        instance_id: reader.fixed()?,
        nonce: reader.fixed()?,
        selected_minor: reader.u16()?,
        capabilities: reader.u64()?,
    };
    reader.finish()?;
    if response.instance_id == [0; 16]
        || response.nonce == [0; 16]
        || !valid_minor(response.selected_minor)
        || response.capabilities & !HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT != 0
        || (response.selected_minor == HOLDER_PROTOCOL_BASELINE_MINOR && response.capabilities != 0)
    {
        return Err(HolderProtocolError::InvalidField);
    }
    Ok(response)
}

fn valid_minor(minor: u16) -> bool {
    (HOLDER_PROTOCOL_BASELINE_MINOR..=HOLDER_PROTOCOL_MINOR).contains(&minor)
}
