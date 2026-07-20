use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{PersistError, Result};

mod environment;
mod io;
mod unix;
pub use environment::{
    decode_environment_snapshot, encode_environment_snapshot, EnvironmentCaptureStatus,
    EnvironmentPolicy, EnvironmentSnapshot, ShellLaunchEnvironment, MAX_SHELL_ENVIRONMENT_BYTES,
    MAX_SHELL_ENVIRONMENT_NAME_BYTES, MAX_SHELL_ENVIRONMENT_VALUE_BYTES,
    MAX_SHELL_ENVIRONMENT_VARIABLES, SHELL_ENVIRONMENT_FORMAT_VERSION,
    SHELL_ENVIRONMENT_POLICY_VERSION, STATE_ENV_INCLUDE, STATE_ENV_MAX_BYTES,
    STATE_ENV_MAX_VARIABLES, STATE_ENV_POLICY_FINGERPRINT,
};
pub use io::{create_identity, read_validated, remove_validated, write_atomic};
#[cfg(test)]
use unix::validate_private_attributes;

pub const SHELL_STATE_VERSION: u32 = 2;
pub const SHELL_STATE_LEGACY_VERSION: u32 = 1;
pub const MAX_SHELL_STATE_BYTES: usize = 72 * 1024;
pub const MAX_SHELL_CWD_BYTES: usize = 4096;
const MAX_STATE_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellStateIdentity {
    session_id: u32,
    incarnation: [u8; 16],
    state_file: PathBuf,
}

impl ShellStateIdentity {
    pub fn session_id(&self) -> u32 {
        self.session_id
    }

    pub fn incarnation(&self) -> [u8; 16] {
        self.incarnation
    }

    pub fn incarnation_hex(&self) -> String {
        encode_incarnation(self.incarnation)
    }

    pub fn path(&self) -> &Path {
        &self.state_file
    }

    pub fn path_string(&self) -> String {
        self.state_file
            .to_str()
            .expect("validated state path must remain UTF-8")
            .to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellStateEnvelope {
    pub version: u32,
    pub session_id: u32,
    pub incarnation: String,
    pub sequence: u64,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentSnapshot>,
}

impl ShellStateEnvelope {
    pub fn new(session_id: u32, incarnation: [u8; 16], sequence: u64, cwd: String) -> Result<Self> {
        let state = Self {
            version: SHELL_STATE_LEGACY_VERSION,
            session_id,
            incarnation: encode_incarnation(incarnation),
            sequence,
            cwd,
            environment: None,
        };
        validate_state_fields(&state)?;
        Ok(state)
    }

    pub fn new_v2(
        session_id: u32,
        incarnation: [u8; 16],
        sequence: u64,
        cwd: String,
        environment: EnvironmentSnapshot,
    ) -> Result<Self> {
        let state = Self {
            version: SHELL_STATE_VERSION,
            session_id,
            incarnation: encode_incarnation(incarnation),
            sequence,
            cwd,
            environment: Some(environment),
        };
        validate_state_fields(&state)?;
        Ok(state)
    }
}

pub fn identity_from_parts(
    session_id: u32,
    incarnation: [u8; 16],
    state_file: PathBuf,
) -> Result<ShellStateIdentity> {
    if session_id == 0 {
        return Err(invalid("shell state session id must be non-zero"));
    }
    if incarnation == [0; 16] {
        return Err(invalid("shell state incarnation must be non-zero"));
    }
    let path = state_file
        .to_str()
        .ok_or_else(|| invalid("shell state path must be valid UTF-8"))?;
    if !state_file.is_absolute() || path.len() > MAX_STATE_PATH_BYTES || path.contains('\0') {
        return Err(invalid("shell state path is invalid"));
    }
    let expected = format!("{session_id}-{}.json", encode_incarnation(incarnation));
    if state_file.file_name().and_then(|name| name.to_str()) != Some(expected.as_str())
        || state_file
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some("session-state")
    {
        return Err(invalid("shell state path does not match identity"));
    }
    Ok(ShellStateIdentity {
        session_id,
        incarnation,
        state_file,
    })
}

pub fn encode_envelope(state: &ShellStateEnvelope) -> Result<Vec<u8>> {
    validate_state_fields(state)?;
    let encoded = serde_json::to_vec(state)
        .map_err(|error| invalid(format!("encode shell state: {error}")))?;
    if encoded.len() > MAX_SHELL_STATE_BYTES {
        return Err(invalid("shell state envelope exceeds size limit"));
    }
    Ok(encoded)
}

pub fn decode_and_validate(
    identity: &ShellStateIdentity,
    minimum_sequence: u64,
    encoded: &[u8],
) -> Result<ShellStateEnvelope> {
    if encoded.len() > MAX_SHELL_STATE_BYTES {
        return Err(invalid("shell state envelope exceeds size limit"));
    }
    let state: ShellStateEnvelope = serde_json::from_slice(encoded)
        .map_err(|error| invalid(format!("decode shell state: {error}")))?;
    validate_state_fields(&state)?;
    if state.session_id != identity.session_id
        || state.incarnation != identity.incarnation_hex()
        || state.sequence < minimum_sequence
    {
        return Err(invalid("shell state identity or sequence mismatch"));
    }
    Ok(state)
}

fn validate_state_fields(state: &ShellStateEnvelope) -> Result<()> {
    if !matches!(
        (state.version, state.environment.is_some()),
        (SHELL_STATE_LEGACY_VERSION, false) | (SHELL_STATE_VERSION, true)
    ) || state.session_id == 0
        || state.incarnation.len() != 32
        || state.sequence == 0
        || !state
            .incarnation
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || state.incarnation.bytes().all(|byte| byte == b'0')
    {
        return Err(invalid("shell state envelope fields are invalid"));
    }
    if state.cwd.len() > MAX_SHELL_CWD_BYTES
        || state.cwd.contains('\0')
        || !Path::new(&state.cwd).is_absolute()
    {
        return Err(invalid("shell state cwd is invalid"));
    }
    if let Some(environment) = &state.environment {
        environment.validate_hard_limits()?;
    }
    Ok(())
}

fn encode_incarnation(value: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(32);
    for byte in value {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn invalid(message: impl Into<String>) -> PersistError {
    PersistError::invalid_argument(message)
}

#[cfg(test)]
mod environment_tests;
#[cfg(test)]
mod io_tests;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const INCARNATION: [u8; 16] = [0x11; 16];

    fn identity() -> ShellStateIdentity {
        identity_from_parts(
            7,
            INCARNATION,
            PathBuf::from(
                "/run/user/1000/persistshell/session-state/\
                 7-11111111111111111111111111111111.json",
            ),
        )
        .expect("identity")
    }

    #[test]
    fn envelope_round_trip_preserves_valid_state() {
        let identity = identity();
        let state = ShellStateEnvelope::new(7, INCARNATION, 3, "/srv/work".into()).expect("state");

        let encoded = encode_envelope(&state).expect("encode");
        let decoded = decode_and_validate(&identity, 2, &encoded).expect("decode");

        assert_eq!(decoded, state);
        assert_eq!(
            identity.incarnation_hex(),
            "11111111111111111111111111111111"
        );
    }

    #[test]
    fn envelope_rejects_unknown_and_invalid_fields() {
        let identity = identity();
        let unknown = br#"{"version":1,"session_id":7,"incarnation":"11111111111111111111111111111111","sequence":3,"cwd":"/srv","extra":1}"#;
        assert!(decode_and_validate(&identity, 0, unknown).is_err());

        assert!(ShellStateEnvelope::new(0, INCARNATION, 1, "/srv".into()).is_err());
        assert!(ShellStateEnvelope::new(7, [0; 16], 1, "/srv".into()).is_err());
        assert!(ShellStateEnvelope::new(7, INCARNATION, 0, "/srv".into()).is_err());
        assert!(ShellStateEnvelope::new(7, INCARNATION, 1, "relative".into()).is_err());
        assert!(ShellStateEnvelope::new(7, INCARNATION, 1, "/bad\0cwd".into()).is_err());
        assert!(
            ShellStateEnvelope::new(7, INCARNATION, 1, format!("/{}", "x".repeat(4096))).is_err()
        );
    }

    #[test]
    fn envelope_rejects_wrong_identity_version_and_stale_sequence() {
        let identity = identity();
        let mut state = ShellStateEnvelope::new(8, INCARNATION, 3, "/srv".into()).expect("state");
        let encoded = encode_envelope(&state).expect("encode");
        assert!(decode_and_validate(&identity, 0, &encoded).is_err());

        state.session_id = 7;
        state.incarnation = "22222222222222222222222222222222".into();
        let encoded = encode_envelope(&state).expect("encode");
        assert!(decode_and_validate(&identity, 0, &encoded).is_err());

        state.incarnation = identity.incarnation_hex();
        state.version = 2;
        let encoded = serde_json::to_vec(&state).expect("raw encode");
        assert!(decode_and_validate(&identity, 0, &encoded).is_err());

        state.version = 1;
        let encoded = encode_envelope(&state).expect("encode");
        assert!(decode_and_validate(&identity, 4, &encoded).is_err());
    }

    #[test]
    fn envelope_enforces_encoded_size_limit() {
        let identity = identity();
        let oversized = vec![b' '; MAX_SHELL_STATE_BYTES + 1];
        assert!(decode_and_validate(&identity, 0, &oversized).is_err());

        let max_cwd = format!("/{}", "x".repeat(MAX_SHELL_CWD_BYTES - 1));
        let state = ShellStateEnvelope::new(7, INCARNATION, 1, max_cwd).expect("max cwd");
        assert!(encode_envelope(&state).expect("encode").len() <= MAX_SHELL_STATE_BYTES);
    }

    #[test]
    fn identity_rejects_invalid_parts() {
        let path = PathBuf::from("/run/user/1000/persistshell/session-state/state.json");
        assert!(identity_from_parts(0, INCARNATION, path.clone()).is_err());
        assert!(identity_from_parts(7, [0; 16], path.clone()).is_err());
        assert!(identity_from_parts(7, INCARNATION, PathBuf::from("state.json")).is_err());
        assert!(identity_from_parts(7, INCARNATION, path).is_err());
    }
}
