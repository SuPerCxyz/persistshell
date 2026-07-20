use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use persist_ipc::holder::*;

struct Harness {
    child: Child,
    socket: PathBuf,
    control: UnixStream,
    instance: [u8; 16],
    nonce: [u8; 16],
}

impl Harness {
    fn start(root: &Path, ring_size: u32, log_path: Option<String>) -> Self {
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
        let state_dir = root.join("runtime/persistshell/session-state");
        std::fs::create_dir(&state_dir).unwrap();
        std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let nonce = [11; 16];
        let (mut control, instance) = connect_control(&socket, nonce);
        send(
            &mut control,
            HolderMessageType::Create,
            2,
            encode_create_request(&CreateSessionRequest {
                session_id: 1,
                shell: "/bin/sh".into(),
                arguments: Vec::new(),
                cwd: Some(root.to_string_lossy().into_owned()),
                launch_environment: persist_core::shell_state::ShellLaunchEnvironment::legacy(
                    vec![("PS1".into(), "".into())],
                )
                .expect("legacy environment"),
                history_file: None,
                ring_buffer_size: ring_size,
                log_path,
                state_file: root
                    .join(
                        "runtime/persistshell/session-state/\
                         1-11111111111111111111111111111111.json",
                    )
                    .to_string_lossy()
                    .into_owned(),
                state_incarnation: [0x11; 16],
            })
            .unwrap(),
        );
        assert_eq!(
            decode_operation_response(&receive(&mut control).payload)
                .unwrap()
                .status,
            OperationStatus::Ok
        );
        Self {
            child,
            socket,
            control,
            instance,
            nonce,
        }
    }

    fn data(&self, mode: HolderAttachMode) -> UnixStream {
        let mut stream = UnixStream::connect(&self.socket).unwrap();
        send(
            &mut stream,
            HolderMessageType::DataHello,
            1,
            encode_data_hello(&DataHello {
                daemon_pid: std::process::id(),
                instance_id: self.instance,
                nonce: self.nonce,
            }),
        );
        assert_eq!(
            decode_data_hello_ack(&receive(&mut stream).payload)
                .unwrap()
                .status,
            HelloStatus::Accepted
        );
        send(
            &mut stream,
            HolderMessageType::Attach,
            2,
            encode_attach_request(&AttachRequest {
                session_id: 1,
                mode,
                replay_bytes: 64 * 1024,
            }),
        );
        assert_eq!(
            receive(&mut stream).message_type,
            HolderMessageType::AttachResp
        );
        stream
    }

    fn shutdown(mut self) {
        send(
            &mut self.control,
            HolderMessageType::ShutdownAll,
            99,
            Vec::new(),
        );
        assert_eq!(
            receive(&mut self.control).message_type,
            HolderMessageType::ShutdownAllResp
        );
        wait_for_exit(&mut self.child);
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn takeover_resize_signal_and_exit_are_forwarded() {
    let temp = tempfile::tempdir().unwrap();
    let mut harness = Harness::start(temp.path(), 1024 * 1024, None);
    let mut first = harness.data(HolderAttachMode::ReadWrite);
    let mut observer = harness.data(HolderAttachMode::ReadOnly);
    let mut second = harness.data(HolderAttachMode::ReadWrite);

    assert!(wait_for_type(
        &mut first,
        HolderMessageType::WriteRevoked,
        Duration::from_secs(2)
    ));
    send(
        &mut second,
        HolderMessageType::Resize,
        3,
        encode_resize_request(&ResizeRequest {
            rows: 41,
            cols: 101,
        }),
    );
    send(
        &mut second,
        HolderMessageType::Input,
        4,
        b"stty size; printf '__RESIZED__\\n'\n".to_vec(),
    );
    assert!(read_output_until(
        &mut observer,
        b"41 101",
        Duration::from_secs(3)
    ));
    assert!(read_output_until(
        &mut second,
        b"__RESIZED__",
        Duration::from_secs(3)
    ));

    send(
        &mut second,
        HolderMessageType::Input,
        5,
        b"sleep 10\n".to_vec(),
    );
    std::thread::sleep(Duration::from_millis(100));
    send(
        &mut second,
        HolderMessageType::Signal,
        6,
        encode_signal_request(&SignalRequest {
            signal: libc::SIGINT as u32,
        }),
    );
    send(
        &mut second,
        HolderMessageType::Input,
        7,
        b"printf '__AFTER_SIGNAL__\\n'; exit 7\n".to_vec(),
    );
    assert!(read_output_until(
        &mut second,
        b"__AFTER_SIGNAL__",
        Duration::from_secs(3)
    ));
    let exit = wait_for_control_type(
        &mut harness.control,
        HolderMessageType::SessionExited,
        Duration::from_secs(3),
    );
    assert_eq!(
        decode_session_exited_event(&exit.payload)
            .unwrap()
            .exit_code,
        7
    );
    send_operation(&mut harness.control, HolderMessageType::Close, 1);
    assert_eq!(
        receive(&mut harness.control).message_type,
        HolderMessageType::CloseResp
    );
    send_operation(&mut harness.control, HolderMessageType::RetireExited, 1);
    assert_eq!(
        receive(&mut harness.control).message_type,
        HolderMessageType::RetireExitedResp
    );

    send(
        &mut harness.control,
        HolderMessageType::Create,
        20,
        encode_create_request(&CreateSessionRequest {
            session_id: 2,
            shell: "/bin/sh".into(),
            arguments: Vec::new(),
            cwd: Some(temp.path().to_string_lossy().into_owned()),
            launch_environment: persist_core::shell_state::ShellLaunchEnvironment::default(),
            history_file: None,
            ring_buffer_size: 4096,
            log_path: None,
            state_file: temp
                .path()
                .join(
                    "runtime/persistshell/session-state/\
                     2-22222222222222222222222222222222.json",
                )
                .to_string_lossy()
                .into_owned(),
            state_incarnation: [0x22; 16],
        })
        .unwrap(),
    );
    assert_eq!(
        decode_operation_response(&receive(&mut harness.control).payload)
            .unwrap()
            .status,
        OperationStatus::Ok
    );
    send_operation(&mut harness.control, HolderMessageType::Kill, 2);
    assert_eq!(
        receive(&mut harness.control).message_type,
        HolderMessageType::KillResp
    );
    let killed = wait_for_control_type(
        &mut harness.control,
        HolderMessageType::SessionExited,
        Duration::from_secs(3),
    );
    assert_eq!(
        decode_session_exited_event(&killed.payload)
            .unwrap()
            .exit_code,
        128 + libc::SIGKILL
    );
    send_operation(&mut harness.control, HolderMessageType::Close, 2);
    assert_eq!(
        receive(&mut harness.control).message_type,
        HolderMessageType::CloseResp
    );
    send_operation(&mut harness.control, HolderMessageType::RetireExited, 2);
    assert_eq!(
        receive(&mut harness.control).message_type,
        HolderMessageType::RetireExitedResp
    );
    send(
        &mut harness.control,
        HolderMessageType::Inventory,
        21,
        encode_inventory_request(&InventoryRequest {
            cursor: 0,
            limit: 16,
        }),
    );
    assert!(
        decode_inventory_response(&receive(&mut harness.control).payload)
            .unwrap()
            .entries
            .is_empty()
    );
    harness.shutdown();
}

#[test]
fn large_output_is_bounded_and_log_worker_persists_output() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("session.log");
    let harness = Harness::start(
        temp.path(),
        64 * 1024,
        Some(log_path.to_string_lossy().into_owned()),
    );
    let mut slow = harness.data(HolderAttachMode::ReadWrite);
    slow.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    send(
        &mut slow,
        HolderMessageType::Input,
        3,
        b"yes X | head -c 2097152; printf '__LARGE_TAIL__\\n'\n".to_vec(),
    );
    std::thread::sleep(Duration::from_millis(500));

    let mut fresh = harness.data(HolderAttachMode::ReadWrite);
    assert!(read_output_until(
        &mut fresh,
        b"__LARGE_TAIL__",
        Duration::from_secs(5)
    ));
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if std::fs::read(&log_path)
            .is_ok_and(|bytes| bytes.windows(14).any(|window| window == b"__LARGE_TAIL__"))
        {
            harness.shutdown();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("log worker did not persist PTY output");
}

fn connect_control(path: &Path, nonce: [u8; 16]) -> (UnixStream, [u8; 16]) {
    let mut stream = UnixStream::connect(path).unwrap();
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

fn read_output_until(stream: &mut UnixStream, marker: &[u8], timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let frame = receive(stream);
        if frame.message_type == HolderMessageType::Output
            && frame
                .payload
                .windows(marker.len())
                .any(|window| window == marker)
        {
            return true;
        }
    }
    false
}

fn wait_for_type(stream: &mut UnixStream, kind: HolderMessageType, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if receive(stream).message_type == kind {
            return true;
        }
    }
    false
}

fn wait_for_control_type(
    stream: &mut UnixStream,
    kind: HolderMessageType,
    timeout: Duration,
) -> HolderFrame {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let frame = receive(stream);
        if frame.message_type == kind {
            return frame;
        }
    }
    panic!("timed out waiting for control frame");
}

fn send(stream: &mut UnixStream, kind: HolderMessageType, id: u32, payload: Vec<u8>) {
    stream
        .write_all(
            &encode_frame(&HolderFrame {
                message_type: kind,
                flags: 0,
                request_id: id,
                generation: 0,
                payload,
            })
            .unwrap(),
        )
        .unwrap();
}

fn send_operation(stream: &mut UnixStream, kind: HolderMessageType, session_id: u32) {
    send(
        stream,
        kind,
        30 + session_id,
        encode_operation_request(&OperationRequest { session_id }),
    );
}

fn receive(stream: &mut UnixStream) -> HolderFrame {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut header = [0; HOLDER_HEADER_SIZE];
    stream.read_exact(&mut header).unwrap();
    let length = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).unwrap();
    let mut bytes = header.to_vec();
    bytes.extend(payload);
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

fn wait_for_exit(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("holder did not exit");
}
