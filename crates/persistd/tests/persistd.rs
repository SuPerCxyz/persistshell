use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use persist_ipc::{
    decode_attach_resp, decode_list_sessions_resp, decode_new_session_resp, decode_note_get_resp,
    decode_op_resp, decode_process_stats_resp, decode_process_tree_resp, decode_summary_response,
    decode_trend_response, encode_attach, encode_detach, encode_signal, encode_summary_request,
    read_frame, write_frame, AttachPayload, ClientSocket, Completeness, DashboardSummaryRequest,
    DetachPayload, Frame, MessageType, ResizePayload, SignalPayload,
};
use persist_metadata::MetadataStore;

fn wait_for_path(path: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let status = child.try_wait().expect("check daemon");
        let mut stderr = String::new();
        if status.is_some() {
            if let Some(mut output) = child.stderr.take() {
                let _ = output.read_to_string(&mut stderr);
            }
        }
        assert!(
            status.is_none(),
            "daemon exited with {status:?} while waiting for {}: {stderr}",
            path.display()
        );
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {}", path.display());
}

fn stop_daemon(child: &mut Child) {
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().expect("wait daemon").is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    panic!("daemon did not stop after SIGTERM");
}

fn wait_for_pid(path: &Path, child: &mut Child, expected: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        assert!(child.try_wait().expect("check daemon").is_none());
        if std::fs::read_to_string(path)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            == Some(expected)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for daemon pid file");
}

fn read_until_type(stream: &mut std::os::unix::net::UnixStream, expected: MessageType) -> Frame {
    for _ in 0..64 {
        let frame = read_frame(stream).expect("read expected frame");
        if frame.msg_type == expected {
            return frame;
        }
    }
    panic!("did not receive {expected:?}");
}

fn read_until_stdout_contains(
    stream: &mut std::os::unix::net::UnixStream,
    expected: &[u8],
) -> Vec<u8> {
    let mut output = Vec::new();
    for _ in 0..64 {
        let frame = read_frame(stream).expect("read expected stdout");
        if frame.msg_type == MessageType::Stdout {
            output.extend(frame.payload);
            if output
                .windows(expected.len())
                .any(|window| window == expected)
            {
                return output;
            }
        }
    }
    panic!("did not receive expected stdout");
}

#[test]
fn persistd_help_prints_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_persistd"))
        .arg("help")
        .output()
        .expect("run persistd help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.contains("Usage"));
}

#[test]
fn persistd_unknown_command_uses_error_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_persistd"))
        .arg("wat")
        .output()
        .expect("run persistd wat");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(stderr.contains("invalid argument"));
    assert!(stderr.contains("unknown persistd command"));
}

#[test]
fn persistd_unknown_command_shows_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_persistd"))
        .arg("wat")
        .output()
        .expect("run persistd wat");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(stderr.contains("invalid argument"));
    assert!(stderr.contains("unknown persistd command"));
}

#[test]
fn foreground_serves_ipc_and_cleans_up_on_sigterm() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let runtime = temp.path().join("runtime");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&runtime).expect("runtime");
    let holder_source = Path::new(env!("CARGO_BIN_EXE_persistd"))
        .parent()
        .expect("binary directory")
        .join("persist-holder");
    let holder_binary = temp.path().join("persist-holder");
    let holder_available = holder_source.exists();
    if holder_available {
        std::fs::copy(&holder_source, &holder_binary).expect("copy holder binary");
        std::fs::set_permissions(&holder_binary, std::fs::Permissions::from_mode(0o700))
            .expect("secure holder binary");
    }

    let mut command = Command::new(env!("CARGO_BIN_EXE_persistd"));
    command
        .arg("foreground")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env("XDG_DATA_HOME", temp.path().join("data"))
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .env("PERSIST_HOLDER_PATH", &holder_binary)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if holder_available {
        command.env("PERSIST_HOLDER_PATH", &holder_binary);
    }
    let mut daemon = command.spawn().expect("start daemon");
    let socket_path = runtime.join("persistshell/persist.sock");
    let pid_path = runtime.join("persistshell/daemon.pid");
    wait_for_path(&socket_path, &mut daemon);
    assert!(pid_path.exists());
    if holder_available {
        wait_for_path(&runtime.join("persistshell/holder.sock"), &mut daemon);
        assert!(runtime.join("persistshell/holder.pid").exists());
    }
    let metrics_path = temp.path().join("state/persistshell/metrics");
    wait_for_path(&metrics_path, &mut daemon);
    assert_eq!(
        std::fs::metadata(&metrics_path)
            .expect("metrics metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    let mut client = ClientSocket::connect(&socket_path).expect("connect");
    let ack = client
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("hello");
    assert_eq!(ack.status, persist_ipc::HelloStatus::Accepted);
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::DashboardSummary,
            flags: 0,
            request_id: 41,
            payload: encode_summary_request(&DashboardSummaryRequest {
                cursor: 0,
                limit: 128,
            }),
        },
    )
    .expect("dashboard summary");
    let response = read_frame(client.stream()).expect("dashboard summary response");
    assert_eq!(response.msg_type, MessageType::DashboardSummaryResp);
    assert_eq!(response.request_id, 41);
    let summary = decode_summary_response(&response.payload).expect("decode dashboard summary");
    assert!(summary.sessions.len() <= 128);

    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::DashboardTrend,
            flags: 0,
            request_id: 42,
            payload: vec![0xFF],
        },
    )
    .expect("invalid dashboard trend");
    let response = read_frame(client.stream()).expect("invalid dashboard trend response");
    assert_eq!(response.msg_type, MessageType::DashboardTrendResp);
    assert_eq!(response.request_id, 42);
    let trend = decode_trend_response(&response.payload).expect("decode unavailable trend");
    assert_eq!(trend.completeness, Completeness::Unavailable);
    assert!(trend.points.is_empty());

    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::NewSession,
            flags: 0,
            request_id: 1,
            payload: vec![],
        },
    )
    .expect("new session");
    let response = read_frame(client.stream()).expect("new session response");
    assert_eq!(response.msg_type, MessageType::NewSessionResp);
    let session = decode_new_session_resp(&response.payload).expect("decode new session");
    assert!(session.session_id >= 1);
    let metadata = MetadataStore::open(&temp.path().join("data/persistshell/metadata.db"))
        .expect("open metadata");
    assert!(metadata
        .get_session(session.session_id)
        .expect("read metadata")
        .is_some());

    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::ListSessions,
            flags: 0,
            request_id: 7,
            payload: vec![],
        },
    )
    .expect("list running session");
    let response = read_frame(client.stream()).expect("running list response");
    let list = decode_list_sessions_resp(&response.payload).expect("decode running list");
    let running = list
        .sessions
        .iter()
        .find(|entry| entry.session_id == session.session_id)
        .expect("running session entry");
    assert!(running.foreground_pid.is_some());
    assert!(!running.foreground_name.is_empty());

    let session_payload = encode_detach(&DetachPayload {
        session_id: session.session_id,
    });
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::ProcessTree,
            flags: 0,
            request_id: 8,
            payload: session_payload.clone(),
        },
    )
    .expect("process tree");
    let tree = read_frame(client.stream()).expect("process tree response");
    assert!(!decode_process_tree_resp(&tree.payload)
        .expect("decode process tree")
        .nodes
        .is_empty());

    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::ProcessStats,
            flags: 0,
            request_id: 9,
            payload: session_payload.clone(),
        },
    )
    .expect("process stats");
    let stats = read_frame(client.stream()).expect("process stats response");
    assert!(decode_process_stats_resp(&stats.payload)
        .expect("decode process stats")
        .pid
        .is_some());

    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::SessionSnapshot,
            flags: 0,
            request_id: 10,
            payload: session_payload,
        },
    )
    .expect("session snapshot");
    let snapshot = read_frame(client.stream()).expect("session snapshot response");
    let snapshot = decode_note_get_resp(&snapshot.payload).expect("decode snapshot");
    let snapshot: serde_json::Value = serde_json::from_str(&snapshot).expect("snapshot json");
    assert!(snapshot["foreground_pid"].as_u64().is_some());
    assert_eq!(snapshot["output_log_state"], "healthy");

    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Metrics,
            flags: 0,
            request_id: 11,
            payload: Vec::new(),
        },
    )
    .expect("metrics");
    let metrics = read_frame(client.stream()).expect("metrics response");
    let metrics = decode_note_get_resp(&metrics.payload).expect("decode metrics");
    let metrics: serde_json::Value = serde_json::from_str(&metrics).expect("metrics json");
    assert_eq!(metrics["holder"]["connected"], true);
    assert!(metrics["holder"]["pid"].as_u64().is_some());
    assert_eq!(
        metrics["holder"]["instance"].as_str().map(str::len),
        Some(32)
    );
    assert_eq!(metrics["sessions"]["log_degraded"], 0);
    assert_eq!(metrics["sessions"]["lost"], 0);

    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Attach,
            flags: 0,
            request_id: 2,
            payload: encode_attach(&AttachPayload {
                session_id: session.session_id,
            }),
        },
    )
    .expect("attach session");
    let response = read_frame(client.stream()).expect("attach response");
    let attach = decode_attach_resp(&response.payload).expect("decode attach");
    assert!(attach.ok, "attach failed: {}", attach.error_msg);
    client
        .stream()
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("attach read timeout");
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Ping,
            flags: 0,
            request_id: 11,
            payload: Vec::new(),
        },
    )
    .expect("ping attached session");
    read_until_type(client.stream(), MessageType::Pong);
    let mut metrics_client = ClientSocket::connect(&socket_path).expect("connect metrics client");
    metrics_client
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("metrics hello");
    write_frame(
        metrics_client.stream(),
        &Frame {
            msg_type: MessageType::Metrics,
            flags: 0,
            request_id: 12,
            payload: Vec::new(),
        },
    )
    .expect("attached metrics");
    let metrics = read_frame(metrics_client.stream()).expect("attached metrics response");
    let metrics = decode_note_get_resp(&metrics.payload).expect("decode attached metrics");
    let metrics: serde_json::Value = serde_json::from_str(&metrics).expect("attached metrics json");
    assert_eq!(metrics["sessions"]["active_writers"], 1);
    drop(metrics_client);
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Signal,
            flags: 0,
            request_id: 12,
            payload: encode_signal(&SignalPayload {
                session_id: session.session_id,
                signal: libc::SIGCONT as u32,
            }),
        },
    )
    .expect("signal attached session");
    let signal = read_until_type(client.stream(), MessageType::SignalResp);
    assert!(
        decode_op_resp(&signal.payload)
            .expect("decode signal response")
            .ok
    );

    let mut readonly = ClientSocket::connect(&socket_path).expect("connect readonly client");
    readonly
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("readonly hello");
    write_frame(
        readonly.stream(),
        &Frame {
            msg_type: MessageType::AttachReadOnly,
            flags: 0,
            request_id: 13,
            payload: encode_attach(&AttachPayload {
                session_id: session.session_id,
            }),
        },
    )
    .expect("readonly attach");
    let response = read_frame(readonly.stream()).expect("readonly attach response");
    assert!(
        decode_attach_resp(&response.payload)
            .expect("decode readonly attach")
            .ok
    );

    let mut takeover = ClientSocket::connect(&socket_path).expect("connect takeover client");
    takeover
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("takeover hello");
    write_frame(
        takeover.stream(),
        &Frame {
            msg_type: MessageType::Attach,
            flags: 0,
            request_id: 14,
            payload: encode_attach(&AttachPayload {
                session_id: session.session_id,
            }),
        },
    )
    .expect("takeover attach");
    let response = read_frame(takeover.stream()).expect("takeover attach response");
    assert!(
        decode_attach_resp(&response.payload)
            .expect("decode takeover attach")
            .ok
    );
    takeover
        .stream()
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("takeover read timeout");
    read_until_type(client.stream(), MessageType::WriteRevoked);
    read_until_type(takeover.stream(), MessageType::WriteGranted);
    write_frame(
        takeover.stream(),
        &Frame {
            msg_type: MessageType::Resize,
            flags: 0,
            request_id: 15,
            payload: persist_ipc::encode_resize(&ResizePayload { rows: 37, cols: 91 }),
        },
    )
    .expect("resize takeover session");
    write_frame(
        takeover.stream(),
        &Frame {
            msg_type: MessageType::Stdin,
            flags: 0,
            request_id: 3,
            payload: b"cd /; sleep 1; exit\n".to_vec(),
        },
    )
    .expect("exit shell");

    let deadline = Instant::now() + Duration::from_secs(5);
    while metadata
        .get_session(session.session_id)
        .expect("read metadata")
        .expect("session metadata")
        .status
        != "closed"
    {
        assert!(Instant::now() < deadline, "shell did not close");
        std::thread::sleep(Duration::from_millis(25));
    }
    drop(client);
    drop(readonly);
    drop(takeover);
    let mut list_client = ClientSocket::connect(&socket_path).expect("connect list client");
    list_client
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("list hello");
    write_frame(
        list_client.stream(),
        &Frame {
            msg_type: MessageType::ListSessions,
            flags: 0,
            request_id: 4,
            payload: vec![],
        },
    )
    .expect("list sessions");
    let response = read_frame(list_client.stream()).expect("list response");
    let sessions = decode_list_sessions_resp(&response.payload).expect("decode list");
    assert!(sessions
        .sessions
        .iter()
        .any(|entry| entry.session_id == session.session_id && entry.status == "closed"));
    drop(list_client);

    let closed = metadata
        .get_session(session.session_id)
        .expect("read closed metadata")
        .expect("closed session metadata");
    assert_eq!(closed.cwd.as_deref(), Some("/"));
    assert!(closed.env_snapshot.is_some());

    let mut recovery_client = ClientSocket::connect(&socket_path).expect("connect recovery client");
    recovery_client
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("recovery hello");
    write_frame(
        recovery_client.stream(),
        &Frame {
            msg_type: MessageType::Attach,
            flags: 0,
            request_id: 5,
            payload: encode_attach(&AttachPayload {
                session_id: session.session_id,
            }),
        },
    )
    .expect("attach closed session");
    let response = read_frame(recovery_client.stream()).expect("closed attach response");
    assert!(
        decode_attach_resp(&response.payload)
            .expect("decode closed attach")
            .ok
    );
    assert_eq!(
        metadata
            .get_session(session.session_id)
            .expect("read reopened metadata")
            .expect("reopened session metadata")
            .status,
        "running"
    );
    drop(recovery_client);

    let mut operation_client =
        ClientSocket::connect(&socket_path).expect("connect holder operation client");
    operation_client
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("operation hello");
    let operation_payload = encode_detach(&DetachPayload {
        session_id: session.session_id,
    });
    write_frame(
        operation_client.stream(),
        &Frame {
            msg_type: MessageType::Close,
            flags: 0,
            request_id: 16,
            payload: operation_payload,
        },
    )
    .expect("close holder session");
    let response = read_frame(operation_client.stream()).expect("close response");
    let close = decode_op_resp(&response.payload).expect("decode close");
    assert!(close.ok, "close failed: {}", close.error_msg);
    assert_eq!(
        metadata
            .get_session(session.session_id)
            .expect("read explicitly closed metadata")
            .expect("explicitly closed session metadata")
            .status,
        "closed"
    );

    write_frame(
        operation_client.stream(),
        &Frame {
            msg_type: MessageType::NewSession,
            flags: 0,
            request_id: 17,
            payload: Vec::new(),
        },
    )
    .expect("create kill session");
    let response = read_frame(operation_client.stream()).expect("kill session response");
    let kill_session = decode_new_session_resp(&response.payload).expect("decode kill session");
    write_frame(
        operation_client.stream(),
        &Frame {
            msg_type: MessageType::Kill,
            flags: 0,
            request_id: 18,
            payload: encode_detach(&DetachPayload {
                session_id: kill_session.session_id,
            }),
        },
    )
    .expect("kill holder session");
    let response = read_frame(operation_client.stream()).expect("kill response");
    assert!(decode_op_resp(&response.payload).expect("decode kill").ok);
    assert_eq!(
        metadata
            .get_session(kill_session.session_id)
            .expect("read killed metadata")
            .expect("killed session metadata")
            .status,
        "closed"
    );
    drop(operation_client);

    let duplicate = Command::new(env!("CARGO_BIN_EXE_persistd"))
        .arg("foreground")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env("XDG_DATA_HOME", temp.path().join("data"))
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .output()
        .expect("start duplicate daemon");
    assert!(!duplicate.status.success());

    stop_daemon(&mut daemon);
    if holder_available {
        assert!(!runtime.join("persistshell/holder.sock").exists());
        assert!(!runtime.join("persistshell/holder.pid").exists());
    }
    assert!(!socket_path.exists());
    assert!(!pid_path.exists());

    let mut restarted = Command::new(env!("CARGO_BIN_EXE_persistd"))
        .arg("foreground")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env("XDG_DATA_HOME", temp.path().join("data"))
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .env("PERSIST_HOLDER_PATH", &holder_binary)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("restart daemon");
    wait_for_path(&socket_path, &mut restarted);
    let mut client = ClientSocket::connect(&socket_path).expect("reconnect");
    client
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("second hello");
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::NewSession,
            flags: 0,
            request_id: 2,
            payload: vec![],
        },
    )
    .expect("second new session");
    let response = read_frame(client.stream()).expect("second new session response");
    let second_session = decode_new_session_resp(&response.payload).expect("decode second");
    assert_eq!(second_session.session_id, session.session_id + 2);
    drop(client);
    stop_daemon(&mut restarted);
    assert!(!socket_path.exists());
    assert!(!pid_path.exists());
}

#[test]
fn daemon_crash_leaves_holder_for_next_daemon_claim() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let runtime = temp.path().join("runtime");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let source = Path::new(env!("CARGO_BIN_EXE_persistd"))
        .parent()
        .unwrap()
        .join("persist-holder");
    if !source.exists() {
        return;
    }
    let holder_binary = temp.path().join("persist-holder");
    std::fs::copy(source, &holder_binary).unwrap();
    std::fs::set_permissions(&holder_binary, std::fs::Permissions::from_mode(0o700)).unwrap();

    let start = || {
        Command::new(env!("CARGO_BIN_EXE_persistd"))
            .arg("foreground")
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .env("XDG_DATA_HOME", temp.path().join("data"))
            .env("XDG_STATE_HOME", temp.path().join("state"))
            .env("PERSIST_HOLDER_PATH", &holder_binary)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    };
    let pid_path = runtime.join("persistshell/daemon.pid");
    let holder_pid_path = runtime.join("persistshell/holder.pid");
    let holder_socket = runtime.join("persistshell/holder.sock");
    let mut first = start();
    let first_pid = first.id();
    wait_for_pid(&pid_path, &mut first, first_pid);
    wait_for_path(&holder_socket, &mut first);
    let holder_pid = std::fs::read_to_string(&holder_pid_path)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let holder_inode = std::fs::metadata(&holder_socket).unwrap().ino();

    let public_socket = runtime.join("persistshell/persist.sock");
    wait_for_path(&public_socket, &mut first);
    let mut client = ClientSocket::connect(&public_socket).expect("connect first daemon");
    client
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("first daemon hello");
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::NewSession,
            flags: 0,
            request_id: 1,
            payload: Vec::new(),
        },
    )
    .expect("create crash-surviving session");
    let response = read_frame(client.stream()).expect("new crash session response");
    let session = decode_new_session_resp(&response.payload).expect("decode crash session");
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Attach,
            flags: 0,
            request_id: 2,
            payload: encode_attach(&AttachPayload {
                session_id: session.session_id,
            }),
        },
    )
    .expect("attach crash session");
    let response = read_frame(client.stream()).expect("crash attach response");
    assert!(
        decode_attach_resp(&response.payload)
            .expect("decode crash attach")
            .ok
    );
    client
        .stream()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("crash session timeout");
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Stdin,
            flags: 0,
            request_id: 3,
            payload: b"stty -echo; echo holder_ready\n".to_vec(),
        },
    )
    .expect("prepare crash-surviving shell");
    read_until_stdout_contains(client.stream(), b"holder_ready");
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Stdin,
            flags: 0,
            request_id: 4,
            payload: b"sleep 1; echo holder_survived\n".to_vec(),
        },
    )
    .expect("start crash-surviving command");
    std::thread::sleep(Duration::from_millis(50));

    unsafe { libc::kill(first_pid as i32, libc::SIGKILL) };
    first.wait().unwrap();
    drop(client);
    assert_eq!(unsafe { libc::kill(holder_pid, 0) }, 0);
    assert_eq!(
        std::fs::metadata(&holder_socket).unwrap().ino(),
        holder_inode
    );
    std::thread::sleep(Duration::from_millis(1200));

    let mut second = start();
    let second_pid = second.id();
    wait_for_pid(&pid_path, &mut second, second_pid);
    assert_eq!(
        std::fs::read_to_string(&holder_pid_path)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap(),
        holder_pid
    );
    assert_eq!(
        std::fs::metadata(&holder_socket).unwrap().ino(),
        holder_inode
    );
    let mut recovered = ClientSocket::connect(&public_socket).expect("connect recovered daemon");
    recovered
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("recovered daemon hello");
    write_frame(
        recovered.stream(),
        &Frame {
            msg_type: MessageType::AttachReadOnly,
            flags: 0,
            request_id: 5,
            payload: encode_attach(&AttachPayload {
                session_id: session.session_id,
            }),
        },
    )
    .expect("attach recovered session");
    let response = read_frame(recovered.stream()).expect("recovered attach response");
    assert!(
        decode_attach_resp(&response.payload)
            .expect("decode recovered attach")
            .ok
    );
    recovered
        .stream()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("recovered replay timeout");
    let replay = read_until_stdout_contains(recovered.stream(), b"holder_survived");
    assert!(String::from_utf8_lossy(&replay).contains("holder_survived"));

    let mut first_writer = ClientSocket::connect(&public_socket).expect("connect first writer");
    first_writer
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("first writer hello");
    first_writer
        .stream()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("first writer timeout");
    write_frame(
        first_writer.stream(),
        &Frame {
            msg_type: MessageType::Attach,
            flags: 0,
            request_id: 6,
            payload: encode_attach(&AttachPayload {
                session_id: session.session_id,
            }),
        },
    )
    .expect("attach first recovered writer");
    assert!(
        decode_attach_resp(&read_frame(first_writer.stream()).unwrap().payload)
            .unwrap()
            .ok
    );

    let mut takeover = ClientSocket::connect(&public_socket).expect("connect recovered takeover");
    takeover
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("recovered takeover hello");
    takeover
        .stream()
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("takeover timeout");
    write_frame(
        takeover.stream(),
        &Frame {
            msg_type: MessageType::Attach,
            flags: 0,
            request_id: 7,
            payload: encode_attach(&AttachPayload {
                session_id: session.session_id,
            }),
        },
    )
    .expect("attach recovered takeover");
    assert!(
        decode_attach_resp(&read_frame(takeover.stream()).unwrap().payload)
            .unwrap()
            .ok
    );
    read_until_type(first_writer.stream(), MessageType::WriteRevoked);
    read_until_type(takeover.stream(), MessageType::WriteGranted);

    write_frame(
        takeover.stream(),
        &Frame {
            msg_type: MessageType::Resize,
            flags: 0,
            request_id: 8,
            payload: persist_ipc::encode_resize(&ResizePayload { rows: 39, cols: 93 }),
        },
    )
    .expect("resize recovered session");
    write_frame(
        takeover.stream(),
        &Frame {
            msg_type: MessageType::Stdin,
            flags: 0,
            request_id: 9,
            payload: b"stty size; echo recovered_resize; sleep 10\n".to_vec(),
        },
    )
    .expect("run recovered signal command");
    let resized = read_until_stdout_contains(recovered.stream(), b"recovered_resize");
    assert!(String::from_utf8_lossy(&resized).contains("39 93"));
    std::thread::sleep(Duration::from_millis(100));
    for (request_id, signal) in [
        (10, libc::SIGTSTP as u32),
        (11, libc::SIGCONT as u32),
        (12, libc::SIGINT as u32),
    ] {
        write_frame(
            takeover.stream(),
            &Frame {
                msg_type: MessageType::Signal,
                flags: 0,
                request_id,
                payload: encode_signal(&SignalPayload {
                    session_id: session.session_id,
                    signal,
                }),
            },
        )
        .expect("signal recovered session");
        let response = read_until_type(takeover.stream(), MessageType::SignalResp);
        assert!(decode_op_resp(&response.payload).unwrap().ok);
    }
    write_frame(
        takeover.stream(),
        &Frame {
            msg_type: MessageType::Stdin,
            flags: 0,
            request_id: 13,
            payload: b"echo recovered_signal\n".to_vec(),
        },
    )
    .expect("write after recovered signals");
    let signaled = read_until_stdout_contains(recovered.stream(), b"recovered_signal");
    assert!(String::from_utf8_lossy(&signaled).contains("recovered_signal"));
    drop(first_writer);
    drop(takeover);
    drop(recovered);
    stop_daemon(&mut second);
    assert!(!holder_socket.exists());
    assert!(!holder_pid_path.exists());
}
