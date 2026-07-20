use super::*;
use std::collections::{BTreeMap, BTreeSet};

use persist_core::shell_state::{
    EnvironmentCaptureStatus, EnvironmentSnapshot, SHELL_ENVIRONMENT_FORMAT_VERSION,
    SHELL_ENVIRONMENT_POLICY_VERSION,
};

fn nonce(value: u8) -> [u8; 16] {
    [value; 16]
}

#[test]
fn frame_minor_is_explicit_and_bounded() {
    let frame = HolderFrame {
        message_type: HolderMessageType::Inventory,
        flags: 0,
        request_id: 4,
        generation: 2,
        payload: encode_inventory_request(&InventoryRequest {
            cursor: 0,
            limit: 1,
        }),
    };
    for minor in [HOLDER_PROTOCOL_BASELINE_MINOR, HOLDER_PROTOCOL_MINOR] {
        let encoded = encode_frame_with_minor(&frame, minor).expect("encode");
        assert_eq!(decode_frame_with_minor(&encoded, minor), Ok(frame.clone()));
    }
    assert_eq!(
        encode_frame_with_minor(&frame, 0),
        Err(HolderProtocolError::VersionMismatch)
    );
    assert_eq!(
        encode_frame_with_minor(&frame, HOLDER_PROTOCOL_MINOR + 1),
        Err(HolderProtocolError::VersionMismatch)
    );
}

#[test]
fn capability_round_trip_binds_instance_nonce_and_minor() {
    let request = CapabilityRequest {
        instance_id: nonce(3),
        nonce: nonce(4),
        max_minor: HOLDER_PROTOCOL_MINOR,
    };
    assert_eq!(
        decode_capability_request(&encode_capability_request(&request)),
        Ok(request)
    );
    let response = CapabilityResponse {
        instance_id: request.instance_id,
        nonce: request.nonce,
        selected_minor: HOLDER_PROTOCOL_MINOR,
        capabilities: HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT,
    };
    let encoded = encode_capability_response(&response);
    assert_eq!(decode_capability_response(&encoded), Ok(response));

    let mut trailing = encoded;
    trailing.push(0);
    assert_eq!(
        decode_capability_response(&trailing),
        Err(HolderProtocolError::Trailing)
    );
}

#[test]
fn environment_exit_context_v2_round_trips_without_changing_legacy_wire() {
    let environment = EnvironmentSnapshot {
        format_version: SHELL_ENVIRONMENT_FORMAT_VERSION,
        policy_version: SHELL_ENVIRONMENT_POLICY_VERSION,
        policy_fingerprint: "0123456789abcdef".into(),
        env_set: BTreeMap::from([("EDITOR".into(), "vim".into())]),
        env_unset: BTreeSet::from(["OLD_EDITOR".into()]),
        capture_status: EnvironmentCaptureStatus::Complete,
    };
    let event = SessionExitedEventV2 {
        session_id: 7,
        exit_code: 0,
        cwd: Some("/srv".into()),
        environment: Some(environment.clone()),
    };
    let encoded = encode_session_exited_event_v2(&event).expect("encode");
    assert_eq!(decode_session_exited_event_v2(&encoded), Ok(event));

    let response = ExitContextResponseV2 {
        session_id: 7,
        status: OperationStatus::Ok,
        exit_code: Some(0),
        cwd: Some("/srv".into()),
        environment: Some(environment),
    };
    let encoded = encode_exit_context_response_v2(&response).expect("encode");
    assert_eq!(decode_exit_context_response_v2(&encoded), Ok(response));

    let legacy = SessionExitedEvent {
        session_id: 7,
        exit_code: 0,
        cwd: Some("/srv".into()),
    };
    assert_eq!(
        decode_session_exited_event(&encode_session_exited_event(&legacy).unwrap()),
        Ok(legacy)
    );
}

#[test]
fn environment_exit_context_v2_rejects_trailing_and_invalid_snapshot() {
    let event = SessionExitedEventV2 {
        session_id: 7,
        exit_code: 0,
        cwd: None,
        environment: None,
    };
    let mut trailing = encode_session_exited_event_v2(&event).expect("encode");
    trailing.push(0);
    assert_eq!(
        decode_session_exited_event_v2(&trailing),
        Err(HolderProtocolError::Trailing)
    );
}

#[test]
fn message_types_have_stable_numbers_and_frame_round_trip() {
    assert_eq!(HOLDER_PROTOCOL_BASELINE_MINOR, 1);
    assert_eq!(HOLDER_PROTOCOL_MINOR, 2);
    assert_eq!(HolderMessageType::Capability as u16, 0x0004);
    assert_eq!(HolderMessageType::CapabilityResp as u16, 0x0005);
    assert_eq!(HolderMessageType::GetExitContext as u16, 0x001a);
    assert_eq!(HolderMessageType::GetExitContextResp as u16, 0x001b);
    assert_eq!(HolderMessageType::RetireExited as u16, 0x001c);
    assert_eq!(HolderMessageType::RetireExitedResp as u16, 0x001d);

    for message_type in [
        HolderMessageType::GetExitContext,
        HolderMessageType::GetExitContextResp,
        HolderMessageType::RetireExited,
        HolderMessageType::RetireExitedResp,
    ] {
        let frame = HolderFrame {
            message_type,
            flags: 0,
            request_id: 9,
            generation: 3,
            payload: Vec::new(),
        };
        assert_eq!(
            decode_frame(&encode_frame(&frame).expect("frame")),
            Ok(frame)
        );
    }
}

#[test]
fn create_state_identity_round_trip_and_bounds() {
    let request = CreateSessionRequest {
        session_id: 21,
        shell: "/bin/bash".into(),
        arguments: vec!["-i".into()],
        cwd: Some("/srv".into()),
        launch_environment: persist_core::shell_state::ShellLaunchEnvironment::default(),
        history_file: None,
        ring_buffer_size: 4096,
        log_path: None,
        state_file: "/run/user/1000/persistshell/session-state/21-state.json".into(),
        state_incarnation: nonce(7),
    };
    let encoded = encode_create_request(&request).expect("encode");
    assert_eq!(decode_create_request(&encoded), Ok(request.clone()));

    let mut invalid = request.clone();
    invalid.state_incarnation = [0; 16];
    assert_eq!(
        encode_create_request(&invalid),
        Err(HolderProtocolError::InvalidField)
    );
    invalid = request.clone();
    invalid.state_file = "relative-state.json".into();
    assert_eq!(
        encode_create_request(&invalid),
        Err(HolderProtocolError::InvalidField)
    );
    invalid.state_file = format!("/{}", "x".repeat(MAX_HOLDER_PATH));
    assert_eq!(
        encode_create_request(&invalid),
        Err(HolderProtocolError::PayloadTooLarge)
    );
}

#[test]
fn inventory_context_flag_matches_exited_state() {
    let exited = HolderSessionEntry {
        session_id: 22,
        shell_pid: 1234,
        state: HolderSessionState::Exited,
        exit_code: Some(0),
        created_at_ms: 10,
        last_active_at_ms: 11,
        ring_bytes: 0,
        writer_active: false,
        log_state: HolderLogState::Disabled,
        exit_context_available: true,
    };
    let response = InventoryResponse {
        entries: vec![exited],
        next_cursor: None,
    };
    assert_eq!(
        decode_inventory_response(&encode_inventory_response(&response).expect("encode")),
        Ok(response)
    );

    let mut running = exited;
    running.state = HolderSessionState::Running;
    running.exit_code = None;
    assert_eq!(
        encode_inventory_response(&InventoryResponse {
            entries: vec![running],
            next_cursor: None,
        }),
        Err(HolderProtocolError::InvalidField)
    );
}

#[test]
fn session_exited_optional_cwd_is_strictly_bounded() {
    let event = SessionExitedEvent {
        session_id: 23,
        exit_code: 17,
        cwd: Some(format!("/{}", "x".repeat(MAX_HOLDER_PATH - 1))),
    };
    let encoded = encode_session_exited_event(&event).expect("encode event");
    assert_eq!(decode_session_exited_event(&encoded), Ok(event.clone()));

    let without_cwd = SessionExitedEvent {
        session_id: 23,
        exit_code: 17,
        cwd: None,
    };
    assert_eq!(
        decode_session_exited_event(
            &encode_session_exited_event(&without_cwd).expect("encode event")
        ),
        Ok(without_cwd)
    );

    let invalid = SessionExitedEvent {
        session_id: 23,
        exit_code: 17,
        cwd: Some("relative".into()),
    };
    assert_eq!(
        encode_session_exited_event(&invalid),
        Err(HolderProtocolError::InvalidField)
    );
    let oversized = SessionExitedEvent {
        cwd: Some(format!("/{}", "x".repeat(MAX_HOLDER_PATH))),
        ..invalid
    };
    assert_eq!(
        encode_session_exited_event(&oversized),
        Err(HolderProtocolError::PayloadTooLarge)
    );
}

#[test]
fn exit_context_response_enforces_status_combinations() {
    let ok = ExitContextResponse {
        session_id: 24,
        status: OperationStatus::Ok,
        exit_code: Some(23),
        cwd: Some("/srv/final".into()),
    };
    let encoded = encode_exit_context_response(&ok).expect("encode response");
    assert_eq!(decode_exit_context_response(&encoded), Ok(ok.clone()));

    let not_found = ExitContextResponse {
        session_id: 24,
        status: OperationStatus::NotFound,
        exit_code: None,
        cwd: None,
    };
    assert_eq!(
        decode_exit_context_response(
            &encode_exit_context_response(&not_found).expect("encode response")
        ),
        Ok(not_found)
    );

    for invalid in [
        ExitContextResponse {
            session_id: 24,
            status: OperationStatus::Ok,
            exit_code: None,
            cwd: None,
        },
        ExitContextResponse {
            session_id: 24,
            status: OperationStatus::Conflict,
            exit_code: Some(1),
            cwd: None,
        },
        ExitContextResponse {
            session_id: 24,
            status: OperationStatus::Rejected,
            exit_code: None,
            cwd: Some("/wrong".into()),
        },
    ] {
        assert_eq!(
            encode_exit_context_response(&invalid),
            Err(HolderProtocolError::InvalidField)
        );
    }

    let mut trailing = encoded;
    trailing.push(0);
    assert_eq!(
        decode_exit_context_response(&trailing),
        Err(HolderProtocolError::Trailing)
    );
}
