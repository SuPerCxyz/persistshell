use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use persist_ipc::holder::{
    decode_control_hello_ack, decode_frame, encode_control_hello, encode_frame, ControlHello,
    ControlHelloAck, HelloStatus, HolderFrame, HolderMessageType, HOLDER_HEADER_SIZE,
};

struct HolderProcess {
    child: Child,
    socket_path: PathBuf,
    runtime_dir: PathBuf,
}

impl HolderProcess {
    fn start(root: &Path) -> Self {
        let home = root.join("home");
        let runtime = root.join("runtime");
        std::fs::create_dir_all(&home).expect("create home");
        std::fs::create_dir_all(&runtime).expect("create runtime");
        let socket_path = runtime.join("persistshell/holder.sock");
        let previous_inode = std::fs::metadata(&socket_path).ok().map(|meta| meta.ino());
        let child = holder_command(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start holder");
        let runtime_dir = runtime.join("persistshell");
        let mut process = Self {
            child,
            socket_path,
            runtime_dir,
        };
        wait_for_socket(&process.socket_path, &mut process.child, previous_inode);
        process
    }

    fn connect(&self, nonce: [u8; 16]) -> (UnixStream, [u8; 16]) {
        let (stream, ack) = self.connect_status(nonce, std::process::id());
        assert_eq!(
            ack.status,
            HelloStatus::Accepted,
            "control claim with nonce {nonce:?} was rejected"
        );
        (stream, ack.instance_id)
    }

    fn connect_status(&self, nonce: [u8; 16], daemon_pid: u32) -> (UnixStream, ControlHelloAck) {
        let mut stream = connect_with_retry(&self.socket_path);
        let hello = ControlHello {
            uid: unsafe { libc::getuid() },
            daemon_pid,
            nonce,
        };
        write_holder_frame(
            &mut stream,
            HolderFrame {
                message_type: HolderMessageType::ControlHello,
                flags: 0,
                request_id: 1,
                generation: 0,
                payload: encode_control_hello(&hello),
            },
        );
        let response = read_holder_frame(&mut stream);
        assert_eq!(response.message_type, HolderMessageType::ControlHelloAck);
        assert_eq!(response.request_id, 1);
        let ack = decode_control_hello_ack(&response.payload).expect("decode ack");
        assert_eq!(ack.nonce, nonce);
        (stream, ack)
    }

    fn shutdown(mut self) {
        let (mut stream, _) = self.connect([9; 16]);
        write_holder_frame(
            &mut stream,
            HolderFrame {
                message_type: HolderMessageType::ShutdownAll,
                flags: 0,
                request_id: 2,
                generation: 0,
                payload: Vec::new(),
            },
        );
        let response = read_holder_frame(&mut stream);
        assert_eq!(response.message_type, HolderMessageType::ShutdownAllResp);
        wait_for_exit(&mut self.child);
        assert!(!self.socket_path.exists());
        assert!(!self.runtime_dir.join("holder.pid").exists());
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
fn holder_survives_control_disconnect_and_shutdown_cleans_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut process = HolderProcess::start(temp.path());
    assert_eq!(
        std::fs::metadata(&process.runtime_dir)
            .expect("runtime metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&process.socket_path)
            .expect("socket metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let (stream, instance) = process.connect([7; 16]);
    drop(stream);
    std::thread::sleep(Duration::from_millis(50));
    assert!(process.child.try_wait().expect("holder status").is_none());
    let (stream, reconnected_instance) = process.connect([8; 16]);
    assert_eq!(reconnected_instance, instance);
    drop(stream);
    process.shutdown();
}

#[test]
fn duplicate_holder_is_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let process = HolderProcess::start(temp.path());
    let duplicate = holder_command(temp.path())
        .output()
        .expect("run duplicate holder");
    assert!(!duplicate.status.success());
    process.shutdown();
}

#[test]
fn sigterm_cleans_runtime_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut process = HolderProcess::start(temp.path());
    unsafe {
        libc::kill(process.child.id() as i32, libc::SIGTERM);
    }
    wait_for_exit(&mut process.child);
    assert!(!process.socket_path.exists());
    assert!(!process.runtime_dir.join("holder.pid").exists());
}

#[test]
fn forged_pid_and_second_controller_are_rejected_without_stopping_holder() {
    let temp = tempfile::tempdir().expect("tempdir");
    let process = HolderProcess::start(temp.path());
    let (forged, forged_ack) = process.connect_status([3; 16], std::process::id() + 1);
    assert_eq!(forged_ack.status, HelloStatus::PermissionDenied);
    drop(forged);

    let (active, _) = process.connect([4; 16]);
    let (busy, busy_ack) = process.connect_status([5; 16], std::process::id());
    assert_eq!(busy_ack.status, HelloStatus::Busy);
    drop(busy);
    drop(active);
    process.shutdown();
}

#[test]
fn unsafe_runtime_permissions_and_pid_symlink_are_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    prepare_root(temp.path());
    let runtime_dir = temp.path().join("runtime/persistshell");
    std::fs::create_dir(&runtime_dir).expect("create runtime dir");
    std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o755))
        .expect("set unsafe mode");
    assert!(!holder_command(temp.path())
        .output()
        .unwrap()
        .status
        .success());

    std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700))
        .expect("set safe mode");
    let target = temp.path().join("pid-target");
    std::fs::write(&target, b"unchanged").expect("write target");
    std::os::unix::fs::symlink(&target, runtime_dir.join("holder.pid")).expect("symlink pid");
    assert!(!holder_command(temp.path())
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(std::fs::read(&target).unwrap(), b"unchanged");
}

#[test]
fn proven_stale_socket_is_replaced() {
    let temp = tempfile::tempdir().expect("tempdir");
    prepare_root(temp.path());
    let runtime_dir = temp.path().join("runtime/persistshell");
    std::fs::create_dir(&runtime_dir).expect("create runtime dir");
    std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = runtime_dir.join("holder.sock");
    let listener = UnixListener::bind(&socket).expect("bind stale socket");
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    drop(listener);

    let process = HolderProcess::start(temp.path());
    let (stream, _) = process.connect([6; 16]);
    drop(stream);
    process.shutdown();
}

#[test]
fn symlink_runtime_directory_is_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let runtime = temp.path().join("runtime");
    let target = temp.path().join("redirected");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&runtime).expect("create runtime");
    std::fs::create_dir_all(&target).expect("create target");
    std::os::unix::fs::symlink(&target, runtime.join("persistshell")).expect("create symlink");
    let output = holder_command(temp.path())
        .output()
        .expect("run holder with symlink runtime");
    assert!(!output.status.success());
    assert!(!target.join("holder.sock").exists());
}

fn holder_command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_persist-holder"));
    command
        .arg("foreground")
        .env("HOME", root.join("home"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"));
    command
}

fn prepare_root(root: &Path) {
    std::fs::create_dir_all(root.join("home")).expect("create home");
    std::fs::create_dir_all(root.join("runtime")).expect("create runtime");
}

fn wait_for_socket(path: &Path, child: &mut Child, previous_inode: Option<u64>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        assert!(child.try_wait().expect("holder status").is_none());
        let current_inode = std::fs::metadata(path).ok().map(|meta| meta.ino());
        if current_inode.is_some() && current_inode != previous_inode {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {}", path.display());
}

fn wait_for_exit(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().expect("holder status").is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("holder did not exit");
}

fn connect_with_retry(path: &Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::ConnectionRefused
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("connect holder: {error}"),
        }
    }
}

fn write_holder_frame(stream: &mut UnixStream, frame: HolderFrame) {
    let bytes = encode_frame(&frame).expect("encode holder frame");
    stream.write_all(&bytes).expect("write holder frame");
}

fn read_holder_frame(stream: &mut UnixStream) -> HolderFrame {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set timeout");
    let mut header = [0u8; HOLDER_HEADER_SIZE];
    stream.read_exact(&mut header).expect("read holder header");
    let payload_len = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    let mut bytes = Vec::with_capacity(HOLDER_HEADER_SIZE + payload_len);
    bytes.extend_from_slice(&header);
    let mut payload = vec![0u8; payload_len];
    stream
        .read_exact(&mut payload)
        .expect("read holder payload");
    bytes.extend_from_slice(&payload);
    decode_frame(&bytes).expect("decode holder frame")
}
