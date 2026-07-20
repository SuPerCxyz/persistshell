use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use persist_ipc::holder::*;

struct HolderProcess {
    child: Child,
    socket: PathBuf,
}

impl HolderProcess {
    fn start(root: &Path) -> Self {
        for directory in ["home", "runtime", "config", "data", "state"] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        let child = Command::new(env!("CARGO_BIN_EXE_persist-holder"))
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
        let mut process = Self { child, socket };
        wait_for_socket(&process.socket, &mut process.child);
        process
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
        let frame = receive(&mut stream);
        let ack = decode_control_hello_ack(&frame.payload).unwrap();
        assert_eq!(ack.status, HelloStatus::Accepted);
        (stream, ack.instance_id)
    }

    fn data(&self, nonce: [u8; 16], instance_id: [u8; 16]) -> UnixStream {
        let mut stream = UnixStream::connect(&self.socket).unwrap();
        send(
            &mut stream,
            HolderMessageType::DataHello,
            1,
            encode_data_hello(&DataHello {
                daemon_pid: std::process::id(),
                instance_id,
                nonce,
            }),
        );
        let ack = decode_data_hello_ack(&receive(&mut stream).payload).unwrap();
        assert_eq!(ack.status, HelloStatus::Accepted);
        stream
    }

    fn shutdown(mut self, mut control: UnixStream) {
        send(&mut control, HolderMessageType::ShutdownAll, 99, Vec::new());
        assert_eq!(
            receive(&mut control).message_type,
            HolderMessageType::ShutdownAllResp
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.child.try_wait().unwrap().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("holder did not stop");
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
fn real_pty_survives_controller_disconnect_and_replays_output() {
    let temp = tempfile::tempdir().unwrap();
    let process = HolderProcess::start(temp.path());
    let state_dir = temp.path().join("runtime/persistshell/session-state");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let nonce = [7; 16];
    let (mut control, instance_id) = process.control(nonce);

    let request = CreateSessionRequest {
        session_id: 42,
        shell: "/bin/sh".into(),
        arguments: Vec::new(),
        cwd: Some(temp.path().to_string_lossy().into_owned()),
        launch_environment: persist_core::shell_state::ShellLaunchEnvironment::legacy(vec![(
            "PS1".into(),
            "".into(),
        )])
        .expect("legacy environment"),
        history_file: None,
        ring_buffer_size: 1024 * 1024,
        log_path: None,
        state_file: temp
            .path()
            .join(
                "runtime/persistshell/session-state/\
                 42-11111111111111111111111111111111.json",
            )
            .to_string_lossy()
            .into_owned(),
        state_incarnation: [0x11; 16],
    };
    send(
        &mut control,
        HolderMessageType::Create,
        2,
        encode_create_request(&request).unwrap(),
    );
    let created = receive(&mut control);
    assert_eq!(created.message_type, HolderMessageType::CreateResp);
    assert_eq!(
        decode_operation_response(&created.payload).unwrap().status,
        OperationStatus::Ok
    );

    send(
        &mut control,
        HolderMessageType::Inventory,
        3,
        encode_inventory_request(&InventoryRequest {
            cursor: 0,
            limit: 16,
        }),
    );
    let inventory = decode_inventory_response(&receive(&mut control).payload).unwrap();
    assert_eq!(inventory.entries.len(), 1);
    assert_eq!(inventory.entries[0].session_id, 42);

    let mut data = process.data(nonce, instance_id);
    attach(&mut data, 42, HolderAttachMode::ReadWrite);
    send(
        &mut data,
        HolderMessageType::Input,
        4,
        b"printf '__PTY_ECHO__\\n'\n".to_vec(),
    );
    assert!(read_output_until(
        &mut data,
        b"__PTY_ECHO__",
        Duration::from_secs(3)
    ));

    send(
        &mut data,
        HolderMessageType::Input,
        5,
        b"sleep 0.1; printf '__OFFLINE__\\n'\n".to_vec(),
    );
    drop(data);
    drop(control);
    std::thread::sleep(Duration::from_millis(250));

    let reconnect_nonce = [8; 16];
    let (control, reconnected_instance) = process.control(reconnect_nonce);
    assert_eq!(reconnected_instance, instance_id);
    let mut data = process.data(reconnect_nonce, instance_id);
    attach(&mut data, 42, HolderAttachMode::ReadWrite);
    assert!(read_output_until(
        &mut data,
        b"__OFFLINE__",
        Duration::from_secs(3)
    ));
    drop(data);
    process.shutdown(control);
}

fn attach(stream: &mut UnixStream, session_id: u32, mode: HolderAttachMode) {
    send(
        stream,
        HolderMessageType::Attach,
        10,
        encode_attach_request(&AttachRequest {
            session_id,
            mode,
            replay_bytes: 1024 * 1024,
        }),
    );
    let response = receive(stream);
    assert_eq!(response.message_type, HolderMessageType::AttachResp);
    assert_eq!(
        decode_operation_response(&response.payload).unwrap().status,
        OperationStatus::Ok
    );
}

fn read_output_until(stream: &mut UnixStream, marker: &[u8], timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let mut output = Vec::new();
    while Instant::now() < deadline {
        let frame = receive(stream);
        if frame.message_type == HolderMessageType::Output {
            output.extend_from_slice(&frame.payload);
            if output.windows(marker.len()).any(|window| window == marker) {
                return true;
            }
        }
    }
    false
}

fn send(stream: &mut UnixStream, kind: HolderMessageType, request_id: u32, payload: Vec<u8>) {
    let bytes = encode_frame(&HolderFrame {
        message_type: kind,
        flags: 0,
        request_id,
        generation: 0,
        payload,
    })
    .unwrap();
    stream.write_all(&bytes).unwrap();
}

fn receive(stream: &mut UnixStream) -> HolderFrame {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut header = [0u8; HOLDER_HEADER_SIZE];
    stream.read_exact(&mut header).unwrap();
    let payload_len = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    let mut bytes = header.to_vec();
    let mut payload = vec![0; payload_len];
    stream.read_exact(&mut payload).unwrap();
    bytes.extend_from_slice(&payload);
    decode_frame(&bytes).unwrap()
}

fn wait_for_socket(path: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        assert!(child.try_wait().unwrap().is_none());
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("holder socket not ready");
}
