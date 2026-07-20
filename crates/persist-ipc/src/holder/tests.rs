use super::*;

fn nonce(value: u8) -> [u8; 16] {
    [value; 16]
}

#[test]
fn frame_round_trip_and_header_validation() {
    let frame = HolderFrame {
        message_type: HolderMessageType::ControlHello,
        flags: 0,
        request_id: 42,
        generation: 7,
        payload: vec![1, 2, 3],
    };
    let encoded = encode_frame(&frame).expect("encode frame");
    assert_eq!(encoded.len(), HOLDER_HEADER_SIZE + 3);
    assert_eq!(decode_frame(&encoded), Ok(frame));

    let mut bad_magic = encoded.clone();
    bad_magic[0] ^= 0xff;
    assert_eq!(
        decode_frame(&bad_magic),
        Err(HolderProtocolError::InvalidMagic)
    );

    let mut bad_version = encoded.clone();
    bad_version[5] = HOLDER_PROTOCOL_MAJOR as u8 + 1;
    assert_eq!(
        decode_frame(&bad_version),
        Err(HolderProtocolError::VersionMismatch)
    );

    assert_eq!(
        decode_frame(&encoded[..HOLDER_HEADER_SIZE - 1]),
        Err(HolderProtocolError::Truncated)
    );
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert_eq!(decode_frame(&trailing), Err(HolderProtocolError::Trailing));
}

#[test]
fn frame_rejects_unknown_type_reserved_flags_and_oversize() {
    let frame = HolderFrame {
        message_type: HolderMessageType::Output,
        flags: 0,
        request_id: 1,
        generation: 0,
        payload: vec![0; MAX_HOLDER_IO_FRAME],
    };
    assert!(encode_frame(&frame).is_ok());

    let mut unknown = encode_frame(&HolderFrame {
        payload: Vec::new(),
        ..frame.clone()
    })
    .unwrap();
    unknown[12..14].copy_from_slice(&u16::MAX.to_be_bytes());
    assert_eq!(
        decode_frame(&unknown),
        Err(HolderProtocolError::UnknownMessageType)
    );

    let mut flags = unknown;
    flags[12..14].copy_from_slice(&(HolderMessageType::Output as u16).to_be_bytes());
    flags[14..16].copy_from_slice(&1u16.to_be_bytes());
    assert_eq!(decode_frame(&flags), Err(HolderProtocolError::InvalidField));

    let oversized = HolderFrame {
        payload: vec![0; MAX_HOLDER_CONTROL_FRAME + 1],
        ..frame
    };
    assert_eq!(
        encode_frame(&oversized),
        Err(HolderProtocolError::PayloadTooLarge)
    );
}

#[test]
fn control_handshake_round_trip() {
    let hello = ControlHello {
        uid: 1000,
        daemon_pid: 1234,
        nonce: nonce(9),
    };
    assert_eq!(
        decode_control_hello(&encode_control_hello(&hello)),
        Ok(hello)
    );

    let ack = ControlHelloAck {
        holder_pid: 5678,
        instance_id: nonce(4),
        nonce: hello.nonce,
        status: HelloStatus::Accepted,
    };
    assert_eq!(
        decode_control_hello_ack(&encode_control_hello_ack(&ack)),
        Ok(ack)
    );
}

#[test]
fn inventory_round_trip_and_bounds() {
    let request = InventoryRequest {
        cursor: 0,
        limit: MAX_INVENTORY_ENTRIES,
    };
    assert_eq!(
        decode_inventory_request(&encode_inventory_request(&request)),
        Ok(request)
    );

    let entry = HolderSessionEntry {
        session_id: 7,
        shell_pid: 9001,
        state: HolderSessionState::Running,
        exit_code: None,
        created_at_ms: 100,
        last_active_at_ms: 200,
        ring_bytes: 4096,
        writer_active: true,
        log_state: HolderLogState::Healthy,
        exit_context_available: false,
    };
    let response = InventoryResponse {
        entries: vec![entry],
        next_cursor: Some(7),
    };
    assert_eq!(
        decode_inventory_response(&encode_inventory_response(&response).unwrap()),
        Ok(response.clone())
    );

    assert_eq!(
        encode_inventory_request(&InventoryRequest {
            cursor: 0,
            limit: MAX_INVENTORY_ENTRIES + 1,
        }),
        Vec::<u8>::new()
    );
    let mut invalid = encode_inventory_response(&response).unwrap();
    invalid[14] = 9;
    assert_eq!(
        decode_inventory_response(&invalid),
        Err(HolderProtocolError::InvalidField)
    );
}

#[test]
fn create_request_round_trip_and_rejects_unsafe_fields() {
    let request = CreateSessionRequest {
        session_id: 11,
        shell: "/bin/bash".to_string(),
        arguments: vec!["--noprofile".to_string()],
        cwd: Some("/srv/build".to_string()),
        launch_environment: persist_core::shell_state::ShellLaunchEnvironment::legacy(vec![(
            "TERM".to_string(),
            "xterm-256color".to_string(),
        )])
        .expect("legacy environment"),
        history_file: Some("/state/history/11".to_string()),
        ring_buffer_size: 1024 * 1024,
        log_path: Some("/data/logs/11.log".to_string()),
        state_file: "/run/user/1000/persistshell/session-state/11-state.json".to_string(),
        state_incarnation: nonce(5),
    };
    let encoded = encode_create_request(&request).expect("encode create request");
    assert_eq!(decode_create_request(&encoded), Ok(request.clone()));

    let mut invalid = request.clone();
    invalid.session_id = 0;
    assert_eq!(
        encode_create_request(&invalid),
        Err(HolderProtocolError::InvalidField)
    );
    invalid = request.clone();
    invalid.shell = "x".repeat(MAX_HOLDER_PATH + 1);
    assert_eq!(
        encode_create_request(&invalid),
        Err(HolderProtocolError::PayloadTooLarge)
    );
    let too_many = (0..=MAX_HOLDER_ENV_VARS)
        .map(|index| (format!("K{index}"), "v".to_string()))
        .collect();
    assert!(persist_core::shell_state::ShellLaunchEnvironment::legacy(too_many).is_err());
    invalid = request;
    invalid.arguments = vec!["x".to_string(); MAX_HOLDER_ARGUMENTS + 1];
    assert_eq!(
        encode_create_request(&invalid),
        Err(HolderProtocolError::PayloadTooLarge)
    );
}

#[test]
fn create_request_v2_preserves_structured_environment() {
    let request = CreateSessionRequest {
        session_id: 12,
        shell: "/bin/sh".into(),
        arguments: vec![],
        cwd: Some("/srv".into()),
        launch_environment: persist_core::shell_state::ShellLaunchEnvironment::new(
            vec![("EDITOR".into(), "vim".into())],
            vec!["OLD_EDITOR".into()],
            vec![("TERM".into(), "xterm-256color".into())],
            vec![("PERSIST_SESSION_ID".into(), "12".into())],
        )
        .expect("structured environment"),
        history_file: None,
        ring_buffer_size: 4096,
        log_path: None,
        state_file: "/run/user/1000/persistshell/session-state/12-state.json".into(),
        state_incarnation: nonce(6),
    };
    let encoded = encode_create_request_v2(&request).expect("encode v2");
    assert_eq!(decode_create_request_v2(&encoded), Ok(request.clone()));

    let legacy = decode_create_request(
        &encode_create_request(&request).expect("encode legacy compatibility"),
    )
    .expect("decode legacy compatibility");
    assert!(legacy.launch_environment.saved_unset().is_empty());
    assert_eq!(
        legacy.launch_environment.legacy_set_environment(),
        request.launch_environment.legacy_set_environment()
    );
}

#[test]
fn data_handshake_and_attach_round_trip() {
    let hello = DataHello {
        daemon_pid: 1234,
        instance_id: nonce(2),
        nonce: nonce(3),
    };
    assert_eq!(decode_data_hello(&encode_data_hello(&hello)), Ok(hello));
    let ack = DataHelloAck {
        instance_id: hello.instance_id,
        nonce: hello.nonce,
        status: HelloStatus::Accepted,
    };
    assert_eq!(decode_data_hello_ack(&encode_data_hello_ack(&ack)), Ok(ack));

    let attach = AttachRequest {
        session_id: 88,
        mode: HolderAttachMode::ReadWrite,
        replay_bytes: 4096,
    };
    assert_eq!(
        decode_attach_request(&encode_attach_request(&attach)),
        Ok(attach)
    );

    let mut invalid = encode_attach_request(&attach);
    invalid[4] = 7;
    assert_eq!(
        decode_attach_request(&invalid),
        Err(HolderProtocolError::InvalidField)
    );
}

#[test]
fn operation_and_status_payloads_round_trip() {
    let request = OperationRequest { session_id: 17 };
    assert_eq!(
        decode_operation_request(&encode_operation_request(&request)),
        Ok(request)
    );

    let response = OperationResponse {
        session_id: 17,
        status: OperationStatus::Conflict,
        message: "writer already active".to_string(),
    };
    assert_eq!(
        decode_operation_response(&encode_operation_response(&response).unwrap()),
        Ok(response)
    );

    let exited = SessionExitedEvent {
        session_id: 17,
        exit_code: 130,
        cwd: Some("/srv/final".to_string()),
    };
    assert_eq!(
        decode_session_exited_event(&encode_session_exited_event(&exited).expect("encode exited")),
        Ok(exited)
    );

    let degraded = LogDegradedEvent {
        session_id: 17,
        dropped_bytes: 4096,
    };
    assert_eq!(
        decode_log_degraded(&encode_log_degraded(&degraded)),
        Ok(degraded)
    );
}

#[test]
fn resize_signal_and_malformed_payloads_are_bounded() {
    let resize = ResizeRequest {
        rows: 40,
        cols: 120,
    };
    assert_eq!(
        decode_resize_request(&encode_resize_request(&resize)),
        Ok(resize)
    );
    let signal = SignalRequest { signal: 2 };
    assert_eq!(
        decode_signal_request(&encode_signal_request(&signal)),
        Ok(signal)
    );

    assert_eq!(
        decode_operation_request(&[0, 0, 0]),
        Err(HolderProtocolError::Truncated)
    );
    assert_eq!(
        decode_resize_request(&[0, 40, 0, 120, 0]),
        Err(HolderProtocolError::Trailing)
    );
    assert_eq!(
        decode_signal_request(&65u32.to_be_bytes()),
        Err(HolderProtocolError::InvalidField)
    );
    assert_eq!(
        encode_operation_response(&OperationResponse {
            session_id: 1,
            status: OperationStatus::Internal,
            message: "x".repeat(MAX_HOLDER_ERROR_MESSAGE + 1),
        }),
        Err(HolderProtocolError::PayloadTooLarge)
    );
}
