use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use persist_ipc::{
    decode_attach_resp, decode_list_sessions_resp, decode_new_session_resp,
    decode_summary_response, decode_trend_response, encode_attach, encode_summary_request,
    read_frame, write_frame, AttachPayload, ClientSocket, Completeness, DashboardSummaryRequest,
    Frame, MessageType,
};
use persist_metadata::MetadataStore;

fn wait_for_path(path: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        assert!(
            child.try_wait().expect("check daemon").is_none(),
            "daemon exited"
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

    let mut daemon = Command::new(env!("CARGO_BIN_EXE_persistd"))
        .arg("foreground")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env("XDG_DATA_HOME", temp.path().join("data"))
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start daemon");
    let socket_path = runtime.join("persistshell/persist.sock");
    let pid_path = runtime.join("persistshell/daemon.pid");
    wait_for_path(&socket_path, &mut daemon);
    assert!(pid_path.exists());
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
    assert!(
        decode_attach_resp(&response.payload)
            .expect("decode attach")
            .ok
    );
    write_frame(
        client.stream(),
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
    assert!(!socket_path.exists());
    assert!(!pid_path.exists());

    let mut restarted = Command::new(env!("CARGO_BIN_EXE_persistd"))
        .arg("foreground")
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env("XDG_DATA_HOME", temp.path().join("data"))
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
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
    assert_eq!(second_session.session_id, session.session_id + 1);
    drop(client);
    stop_daemon(&mut restarted);
    assert!(!socket_path.exists());
    assert!(!pid_path.exists());
}
