use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use persist_core::shell_state::{
    identity_from_parts, write_atomic, EnvironmentCaptureStatus, EnvironmentSnapshot,
    ShellStateEnvelope, ShellStateIdentity, SHELL_ENVIRONMENT_FORMAT_VERSION,
    SHELL_ENVIRONMENT_POLICY_VERSION,
};
use persist_ipc::holder::*;

const SESSION_ID: u32 = 54;
const INCARNATION: [u8; 16] = [0x54; 16];

struct HolderProcess {
    child: Child,
    socket: PathBuf,
}

impl HolderProcess {
    fn start(root: &Path) -> Self {
        for directory in ["home", "runtime", "config", "data", "state"] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_persist-holder"))
            .arg("foreground")
            .env("HOME", root.join("home"))
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_STATE_HOME", root.join("state"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let socket = root.join("runtime/persistshell/holder.sock");
        wait_for_socket(&socket, &mut child);
        Self { child, socket }
    }

    fn control(&self, nonce: [u8; 16]) -> (UnixStream, [u8; 16]) {
        let mut stream = UnixStream::connect(&self.socket).unwrap();
        send(
            &mut stream,
            HolderMessageType::ControlHello,
            1,
            encode_control_hello(&ControlHello {
                uid: unsafe { libc::getuid() },
                daemon_pid: std::process::id(),
                nonce,
            }),
        );
        let ack = decode_control_hello_ack(&receive(&mut stream).payload).unwrap();
        assert_eq!(ack.status, HelloStatus::Accepted);
        (stream, ack.instance_id)
    }

    fn shutdown(mut self, mut control: UnixStream) {
        send(&mut control, HolderMessageType::ShutdownAll, 99, Vec::new());
        assert_eq!(
            receive(&mut control).message_type,
            HolderMessageType::ShutdownAllResp
        );
        wait_for_exit(&mut self.child);
    }
}

impl Drop for HolderProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn exited_context_survives_controller_disconnect_until_retire() {
    let temp = tempfile::tempdir().unwrap();
    let process = HolderProcess::start(temp.path());
    let identity = create_identity(temp.path());
    let final_cwd = temp.path().join("final");
    std::fs::create_dir(&final_cwd).unwrap();
    write_atomic(
        &identity,
        &ShellStateEnvelope::new_v2(
            SESSION_ID,
            INCARNATION,
            1,
            final_cwd.to_string_lossy().into_owned(),
            EnvironmentSnapshot {
                format_version: SHELL_ENVIRONMENT_FORMAT_VERSION,
                policy_version: SHELL_ENVIRONMENT_POLICY_VERSION,
                policy_fingerprint: "0123456789abcdef".into(),
                env_set: BTreeMap::from([("EDITOR".into(), "vim".into())]),
                env_unset: BTreeSet::from(["OLD_EDITOR".into()]),
                capture_status: EnvironmentCaptureStatus::Complete,
            },
        )
        .unwrap(),
    )
    .unwrap();

    let (mut control, instance) = process.control([0x31; 16]);
    send(
        &mut control,
        HolderMessageType::Create,
        2,
        encode_create_request(&CreateSessionRequest {
            session_id: SESSION_ID,
            shell: "/bin/sh".into(),
            arguments: Vec::new(),
            cwd: Some(temp.path().to_string_lossy().into_owned()),
            launch_environment: persist_core::shell_state::ShellLaunchEnvironment::legacy(vec![(
                "PS1".into(),
                "".into(),
            )])
            .expect("legacy environment"),
            history_file: None,
            ring_buffer_size: 64 * 1024,
            log_path: None,
            state_file: identity.path_string(),
            state_incarnation: INCARNATION,
        })
        .unwrap(),
    );
    assert_eq!(
        operation(&mut control, HolderMessageType::CreateResp).status,
        OperationStatus::Ok
    );

    let mut data = connect_data(&process.socket, instance, [0x31; 16]);
    send(
        &mut data,
        HolderMessageType::Input,
        3,
        b"printf '__EXITING__\\n'; exit 17\n".to_vec(),
    );
    assert!(read_output_until(
        &mut data,
        b"__EXITING__",
        Duration::from_secs(3)
    ));
    drop(control);
    drop(data);

    let nonce = [0x32; 16];
    let (mut control, instance) = process.control(nonce);
    let entry = wait_for_exited(&mut control);
    assert!(entry.exit_context_available);
    assert_eq!(entry.exit_code, Some(17));

    negotiate_v2(&mut control, instance, nonce);
    send_with_minor(
        &mut control,
        HolderMessageType::GetExitContext,
        6,
        encode_operation_request(&OperationRequest {
            session_id: SESSION_ID,
        }),
        HOLDER_PROTOCOL_MINOR,
    );
    let context = decode_exit_context_response_v2(
        &receive_type_with_minor(
            &mut control,
            HolderMessageType::GetExitContextResp,
            HOLDER_PROTOCOL_MINOR,
        )
        .payload,
    )
    .unwrap();
    assert_eq!(context.status, OperationStatus::Ok);
    assert_eq!(context.exit_code, Some(17));
    assert_eq!(context.cwd.as_deref(), final_cwd.to_str());
    assert_eq!(
        context
            .environment
            .as_ref()
            .and_then(|environment| environment.env_set.get("EDITOR"))
            .map(String::as_str),
        Some("vim")
    );
    drop(control);

    let (mut control, _) = process.control([0x33; 16]);
    send_operation(&mut control, HolderMessageType::RetireExited, 7);
    assert_eq!(
        operation(&mut control, HolderMessageType::RetireExitedResp).status,
        OperationStatus::Ok
    );
    assert!(!identity.path().exists());
    assert!(inventory(&mut control).entries.is_empty());
    process.shutdown(control);
}

fn create_identity(root: &Path) -> ShellStateIdentity {
    let state_dir = root.join("runtime/persistshell/session-state");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    identity_from_parts(
        SESSION_ID,
        INCARNATION,
        state_dir.join(format!(
            "{SESSION_ID}-{}.json",
            "54545454545454545454545454545454"
        )),
    )
    .unwrap()
}

fn connect_data(socket: &Path, instance: [u8; 16], nonce: [u8; 16]) -> UnixStream {
    let mut stream = UnixStream::connect(socket).unwrap();
    send(
        &mut stream,
        HolderMessageType::DataHello,
        4,
        encode_data_hello(&DataHello {
            daemon_pid: std::process::id(),
            instance_id: instance,
            nonce,
        }),
    );
    let _ = receive(&mut stream);
    send(
        &mut stream,
        HolderMessageType::Attach,
        5,
        encode_attach_request(&AttachRequest {
            session_id: SESSION_ID,
            mode: HolderAttachMode::ReadWrite,
            replay_bytes: 0,
        }),
    );
    let _ = receive(&mut stream);
    stream
}

fn wait_for_exited(control: &mut UnixStream) -> HolderSessionEntry {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = inventory(control);
        if let Some(entry) = response.entries.into_iter().next() {
            if entry.state == HolderSessionState::Exited {
                return entry;
            }
        }
        assert!(Instant::now() < deadline, "session did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn inventory(control: &mut UnixStream) -> InventoryResponse {
    send(
        control,
        HolderMessageType::Inventory,
        8,
        encode_inventory_request(&InventoryRequest {
            cursor: 0,
            limit: 16,
        }),
    );
    decode_inventory_response(&receive_type(control, HolderMessageType::InventoryResp).payload)
        .unwrap()
}

fn send_operation(stream: &mut UnixStream, kind: HolderMessageType, request_id: u32) {
    send(
        stream,
        kind,
        request_id,
        encode_operation_request(&OperationRequest {
            session_id: SESSION_ID,
        }),
    );
}

fn operation(stream: &mut UnixStream, kind: HolderMessageType) -> OperationResponse {
    decode_operation_response(&receive_type(stream, kind).payload).unwrap()
}

fn send(stream: &mut UnixStream, kind: HolderMessageType, request_id: u32, payload: Vec<u8>) {
    send_with_minor(
        stream,
        kind,
        request_id,
        payload,
        HOLDER_PROTOCOL_BASELINE_MINOR,
    );
}

fn send_with_minor(
    stream: &mut UnixStream,
    kind: HolderMessageType,
    request_id: u32,
    payload: Vec<u8>,
    minor: u16,
) {
    let frame = HolderFrame {
        message_type: kind,
        flags: 0,
        request_id,
        generation: 0,
        payload,
    };
    stream
        .write_all(&encode_frame_with_minor(&frame, minor).unwrap())
        .unwrap();
}

fn receive(stream: &mut UnixStream) -> HolderFrame {
    receive_with_minor(stream, HOLDER_PROTOCOL_BASELINE_MINOR)
}

fn receive_with_minor(stream: &mut UnixStream, minor: u16) -> HolderFrame {
    let mut header = [0u8; HOLDER_HEADER_SIZE];
    stream.read_exact(&mut header).unwrap();
    let payload_len = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    let mut encoded = header.to_vec();
    encoded.resize(HOLDER_HEADER_SIZE + payload_len, 0);
    stream
        .read_exact(&mut encoded[HOLDER_HEADER_SIZE..])
        .unwrap();
    decode_frame_with_minor(&encoded, minor).unwrap()
}

fn receive_type(stream: &mut UnixStream, kind: HolderMessageType) -> HolderFrame {
    loop {
        let frame = receive(stream);
        if frame.message_type == kind {
            return frame;
        }
    }
}

fn receive_type_with_minor(
    stream: &mut UnixStream,
    kind: HolderMessageType,
    minor: u16,
) -> HolderFrame {
    loop {
        let frame = receive_with_minor(stream, minor);
        if frame.message_type == kind {
            return frame;
        }
    }
}

fn negotiate_v2(stream: &mut UnixStream, instance_id: [u8; 16], nonce: [u8; 16]) {
    send_with_minor(
        stream,
        HolderMessageType::Capability,
        9,
        encode_capability_request(&CapabilityRequest {
            instance_id,
            nonce,
            max_minor: HOLDER_PROTOCOL_MINOR,
        }),
        HOLDER_PROTOCOL_MINOR,
    );
    let response = decode_capability_response(
        &receive_type_with_minor(
            stream,
            HolderMessageType::CapabilityResp,
            HOLDER_PROTOCOL_MINOR,
        )
        .payload,
    )
    .unwrap();
    assert_eq!(response.selected_minor, HOLDER_PROTOCOL_MINOR);
}

fn read_output_until(stream: &mut UnixStream, needle: &[u8], timeout: Duration) -> bool {
    stream.set_read_timeout(Some(timeout)).unwrap();
    let deadline = Instant::now() + timeout;
    let mut output = Vec::new();
    while Instant::now() < deadline {
        let frame = receive(stream);
        if frame.message_type == HolderMessageType::Output {
            output.extend_from_slice(&frame.payload);
            if output.windows(needle.len()).any(|window| window == needle) {
                return true;
            }
        }
    }
    false
}

fn wait_for_socket(socket: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        assert!(child.try_wait().unwrap().is_none(), "holder exited early");
        if socket.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("holder socket did not appear");
}

fn wait_for_exit(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("holder did not stop");
}
