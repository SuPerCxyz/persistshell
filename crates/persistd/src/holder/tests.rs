use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use persist_ipc::holder::*;

use super::client::HolderControlClient;

const INSTANCE: [u8; 16] = [3; 16];

#[test]
fn client_negotiates_minor_two_and_environment_capability() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("holder.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        handshake(&mut stream, INSTANCE, None);
        let capability = read_frame_with_minor(&mut stream, HOLDER_PROTOCOL_MINOR);
        let request = decode_capability_request(&capability.payload).unwrap();
        write_frame_with_minor(
            &mut stream,
            &HolderFrame {
                message_type: HolderMessageType::CapabilityResp,
                flags: 0,
                request_id: capability.request_id,
                generation: capability.generation,
                payload: encode_capability_response(&CapabilityResponse {
                    instance_id: INSTANCE,
                    nonce: request.nonce,
                    selected_minor: HOLDER_PROTOCOL_MINOR,
                    capabilities: HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT,
                }),
            },
            HOLDER_PROTOCOL_MINOR,
        );
        let inventory = read_frame_with_minor(&mut stream, HOLDER_PROTOCOL_MINOR);
        write_frame_with_minor(
            &mut stream,
            &HolderFrame {
                message_type: HolderMessageType::InventoryResp,
                flags: 0,
                request_id: inventory.request_id,
                generation: 5,
                payload: encode_inventory_response(&InventoryResponse {
                    entries: vec![],
                    next_cursor: None,
                })
                .unwrap(),
            },
            HOLDER_PROTOCOL_MINOR,
        );
    });

    let client = HolderControlClient::connect(&socket).unwrap();
    assert_eq!(client.protocol_minor(), HOLDER_PROTOCOL_MINOR);
    assert!(client.has_capability(HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT));
    assert!(client.inventory().unwrap().is_empty());
    server.join().unwrap();
}

#[test]
fn capability_probe_disconnect_falls_back_to_same_legacy_holder() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("holder.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = thread::spawn(move || {
        let (mut probe, _) = listener.accept().unwrap();
        handshake(&mut probe, INSTANCE, None);
        let capability = read_frame_with_minor(&mut probe, HOLDER_PROTOCOL_MINOR);
        assert_eq!(capability.message_type, HolderMessageType::Capability);
        drop(probe);

        let (mut legacy, _) = listener.accept().unwrap();
        handshake(&mut legacy, INSTANCE, None);
        let inventory = read_frame(&mut legacy);
        write_response(
            &mut legacy,
            &inventory,
            HolderMessageType::InventoryResp,
            5,
            encode_inventory_response(&InventoryResponse {
                entries: vec![entry(7)],
                next_cursor: None,
            })
            .unwrap(),
        );
    });

    let client = HolderControlClient::connect(&socket).unwrap();
    assert_eq!(client.protocol_minor(), HOLDER_PROTOCOL_BASELINE_MINOR);
    assert!(!client.has_capability(HOLDER_CAPABILITY_ENVIRONMENT_CONTEXT));
    assert_eq!(client.inventory().unwrap(), vec![entry(7)]);
    server.join().unwrap();
}

#[test]
fn client_handshake_inventory_and_shutdown_are_strict() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("holder.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        handshake(&mut stream, INSTANCE, None);
        let first = read_frame(&mut stream);
        assert_eq!(first.message_type, HolderMessageType::Inventory);
        let request = decode_inventory_request(&first.payload).unwrap();
        assert_eq!(request.cursor, 0);
        write_frame(
            &mut stream,
            &HolderFrame {
                message_type: HolderMessageType::SessionExited,
                flags: 0,
                request_id: 0,
                generation: 5,
                payload: encode_session_exited_event(&SessionExitedEvent {
                    session_id: 99,
                    exit_code: 0,
                    cwd: None,
                })
                .unwrap(),
            },
        );
        write_response(
            &mut stream,
            &first,
            HolderMessageType::InventoryResp,
            5,
            encode_inventory_response(&InventoryResponse {
                entries: vec![entry(7)],
                next_cursor: None,
            })
            .unwrap(),
        );
        let shutdown = read_frame(&mut stream);
        assert_eq!(shutdown.message_type, HolderMessageType::ShutdownAll);
        write_response(
            &mut stream,
            &shutdown,
            HolderMessageType::ShutdownAllResp,
            5,
            Vec::new(),
        );
    });

    let client = HolderControlClient::connect_legacy_for_test(&socket).unwrap();
    assert_eq!(client.instance_id(), INSTANCE);
    assert_ne!(client.nonce(), [0; 16]);
    assert_eq!(client.inventory().unwrap(), vec![entry(7)]);
    client.shutdown_all().unwrap();
    server.join().unwrap();
}

#[test]
fn client_rejects_mismatched_response_id() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("holder.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        handshake(&mut stream, INSTANCE, Some(99));
    });
    assert!(HolderControlClient::connect_legacy_for_test(&socket).is_err());
    server.join().unwrap();
}

#[test]
fn reconnect_requires_the_same_holder_instance() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("holder.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        handshake(&mut first, INSTANCE, None);
        assert_eq!(
            read_frame(&mut first).message_type,
            HolderMessageType::Inventory
        );
        drop(first);

        let (mut second, _) = listener.accept().unwrap();
        handshake(&mut second, [9; 16], None);
    });
    let client = HolderControlClient::connect_legacy_for_test(&socket).unwrap();
    assert!(client.inventory().is_err());
    assert!(client.reconnect().is_err());
    server.join().unwrap();
}

#[test]
fn reconnect_to_same_instance_restores_inventory() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("holder.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        handshake(&mut first, INSTANCE, None);
        assert_eq!(
            read_frame(&mut first).message_type,
            HolderMessageType::Inventory
        );
        drop(first);

        let (mut second, _) = listener.accept().unwrap();
        handshake(&mut second, INSTANCE, None);
        let inventory = read_frame(&mut second);
        write_response(
            &mut second,
            &inventory,
            HolderMessageType::InventoryResp,
            6,
            encode_inventory_response(&InventoryResponse {
                entries: vec![entry(8)],
                next_cursor: None,
            })
            .unwrap(),
        );
    });
    let client = HolderControlClient::connect_legacy_for_test(&socket).unwrap();
    assert!(client.inventory().is_err());
    client.reconnect().unwrap();
    assert_eq!(client.inventory().unwrap(), vec![entry(8)]);
    server.join().unwrap();
}

#[test]
fn concurrent_inventory_requests_are_serialized() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("holder.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        handshake(&mut stream, INSTANCE, None);
        for _ in 0..2 {
            let request = read_frame(&mut stream);
            assert_eq!(request.message_type, HolderMessageType::Inventory);
            write_response(
                &mut stream,
                &request,
                HolderMessageType::InventoryResp,
                5,
                encode_inventory_response(&InventoryResponse {
                    entries: Vec::new(),
                    next_cursor: None,
                })
                .unwrap(),
            );
        }
    });
    let client = Arc::new(HolderControlClient::connect_legacy_for_test(&socket).unwrap());
    let first = {
        let client = Arc::clone(&client);
        thread::spawn(move || client.inventory())
    };
    let second = {
        let client = Arc::clone(&client);
        thread::spawn(move || client.inventory())
    };
    assert!(first.join().unwrap().unwrap().is_empty());
    assert!(second.join().unwrap().unwrap().is_empty());
    server.join().unwrap();
}

#[test]
fn exit_context_is_queried_before_explicit_retire() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("holder.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        handshake(&mut stream, INSTANCE, None);

        let context = read_frame(&mut stream);
        assert_eq!(context.message_type, HolderMessageType::GetExitContext);
        assert_ne!(context.request_id, 0);
        assert_eq!(context.generation, 5);
        assert_eq!(
            decode_operation_request(&context.payload)
                .unwrap()
                .session_id,
            7
        );
        write_response(
            &mut stream,
            &context,
            HolderMessageType::GetExitContextResp,
            6,
            encode_exit_context_response(&ExitContextResponse {
                session_id: 7,
                status: OperationStatus::Ok,
                exit_code: Some(23),
                cwd: Some("/srv/final".into()),
            })
            .unwrap(),
        );

        let retire = read_frame(&mut stream);
        assert_eq!(retire.message_type, HolderMessageType::RetireExited);
        assert_eq!(retire.generation, 6);
        write_response(
            &mut stream,
            &retire,
            HolderMessageType::RetireExitedResp,
            7,
            encode_operation_response(&OperationResponse {
                session_id: 7,
                status: OperationStatus::Ok,
                message: String::new(),
            })
            .unwrap(),
        );
    });

    let client = HolderControlClient::connect_legacy_for_test(&socket).unwrap();
    let context = client.exit_context(7).unwrap();
    assert_eq!(context.exit_code, 23);
    assert_eq!(context.cwd.as_deref(), Some("/srv/final"));
    client.retire_exited(7).unwrap();
    server.join().unwrap();
}

#[test]
fn trusted_binary_validation_rejects_relative_and_writable_files() {
    assert!(super::process::validate_binary(Path::new("persist-holder")).is_err());
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("persist-holder");
    std::fs::write(&binary, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(super::process::validate_binary(&binary).is_err());
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(super::process::validate_binary(&binary).is_ok());
}

#[test]
fn release_holder_path_is_fixed_and_ignores_development_overrides() {
    let configured = PathBuf::from("/tmp/configured/persist-holder");
    let sibling = PathBuf::from("/tmp/sibling/persist-holder");

    assert_eq!(
        super::process::select_holder_binary(false, Some(configured), Some(sibling)),
        Some(PathBuf::from("/usr/libexec/persistshell/persist-holder"))
    );
}

#[test]
fn development_holder_path_prefers_explicit_path_then_sibling() {
    let configured = PathBuf::from("/tmp/configured/persist-holder");
    let sibling = PathBuf::from("/tmp/sibling/persist-holder");

    assert_eq!(
        super::process::select_holder_binary(true, Some(configured.clone()), Some(sibling.clone())),
        Some(configured)
    );
    assert_eq!(
        super::process::select_holder_binary(true, None, Some(sibling.clone())),
        Some(sibling)
    );
    assert_eq!(super::process::select_holder_binary(true, None, None), None);
}

fn handshake(stream: &mut UnixStream, instance_id: [u8; 16], response_id: Option<u32>) {
    let hello_frame = read_frame(stream);
    assert_eq!(hello_frame.message_type, HolderMessageType::ControlHello);
    let hello = decode_control_hello(&hello_frame.payload).unwrap();
    write_frame(
        stream,
        &HolderFrame {
            message_type: HolderMessageType::ControlHelloAck,
            flags: 0,
            request_id: response_id.unwrap_or(hello_frame.request_id),
            generation: 5,
            payload: encode_control_hello_ack(&ControlHelloAck {
                holder_pid: std::process::id(),
                instance_id,
                nonce: hello.nonce,
                status: HelloStatus::Accepted,
            }),
        },
    );
}

fn entry(session_id: u32) -> HolderSessionEntry {
    HolderSessionEntry {
        session_id,
        shell_pid: 1000 + session_id,
        state: HolderSessionState::Running,
        exit_code: None,
        created_at_ms: 1,
        last_active_at_ms: 2,
        ring_bytes: 3,
        writer_active: false,
        log_state: HolderLogState::Healthy,
        exit_context_available: false,
    }
}

fn write_response(
    stream: &mut UnixStream,
    request: &HolderFrame,
    kind: HolderMessageType,
    generation: u64,
    payload: Vec<u8>,
) {
    write_frame(
        stream,
        &HolderFrame {
            message_type: kind,
            flags: 0,
            request_id: request.request_id,
            generation,
            payload,
        },
    );
}

fn write_frame(stream: &mut UnixStream, frame: &HolderFrame) {
    stream.write_all(&encode_frame(frame).unwrap()).unwrap();
}

fn write_frame_with_minor(stream: &mut UnixStream, frame: &HolderFrame, minor: u16) {
    stream
        .write_all(&encode_frame_with_minor(frame, minor).unwrap())
        .unwrap();
}

fn read_frame(stream: &mut UnixStream) -> HolderFrame {
    read_frame_with_minor(stream, HOLDER_PROTOCOL_BASELINE_MINOR)
}

fn read_frame_with_minor(stream: &mut UnixStream, minor: u16) -> HolderFrame {
    let mut header = [0u8; HOLDER_HEADER_SIZE];
    stream.read_exact(&mut header).unwrap();
    let length = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).unwrap();
    let mut bytes = header.to_vec();
    bytes.extend(payload);
    decode_frame_with_minor(&bytes, minor).unwrap()
}
