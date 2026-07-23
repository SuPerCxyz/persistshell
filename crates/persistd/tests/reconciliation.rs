use std::io::Read;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use persist_ipc::{
    decode_attach_resp, decode_list_sessions_resp, decode_new_session_resp, decode_note_get_resp,
    decode_op_resp, encode_attach, encode_attach_with_context, encode_detach, encode_lock,
    encode_pin, read_frame, write_frame, AttachPayload, ClientSocket, ConnectionEnvironment,
    DetachPayload, Frame, LockPayload, MessageType, PinPayload,
};
use persist_metadata::MetadataStore;

const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(20);

struct TestEnv {
    temp: tempfile::TempDir,
    home: PathBuf,
    runtime: PathBuf,
    holder: PathBuf,
    helper: PathBuf,
}

impl TestEnv {
    fn new() -> Option<Self> {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let runtime = temp.path().join("runtime");
        let config = temp.path().join("config/persistshell");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&runtime).expect("runtime");
        std::fs::create_dir_all(&config).expect("config");
        std::fs::write(
            config.join("config.toml"),
            "[logging]\nsession_log = true\n\
             [recovery.environment]\ninclude = [\"EDITOR\"]\n",
        )
        .expect("write test config");
        let source = Path::new(env!("CARGO_BIN_EXE_persistd"))
            .parent()
            .expect("binary directory")
            .join("persist-holder");
        if !source.exists() {
            return None;
        }
        let holder = temp.path().join("persist-holder");
        std::fs::copy(source, &holder).expect("copy holder");
        std::fs::set_permissions(&holder, std::fs::Permissions::from_mode(0o700))
            .expect("secure holder");
        let helper = temp.path().join("persist-test-helper");
        std::fs::write(
            &helper,
            b"#!/bin/sh\n\
              if [ \"$1\" = __state-commit ]; then\n\
                IFS= read -r cwd || :\n\
                tmp=\"${PERSIST_STATE_FILE}.test.$$\"\n\
                if [ \"${EDITOR-}\" = persist-stage6 ]; then\n\
                  env_set='{\"EDITOR\":\"persist-stage6\"}'\n\
                  env_unset='[]'\n\
                else\n\
                  env_set='{}'\n\
                  env_unset='[\"EDITOR\"]'\n\
                fi\n\
                printf '{\"version\":2,\"session_id\":%s,\"incarnation\":\"%s\",\"sequence\":%s,\"cwd\":\"%s\",\"environment\":{\"format_version\":1,\"policy_version\":1,\"policy_fingerprint\":\"%s\",\"env_set\":%s,\"env_unset\":%s,\"capture_status\":\"complete\"}}' \\\n\
                  \"$PERSIST_STATE_SESSION_ID\" \"$PERSIST_STATE_INCARNATION\" \\\n\
                  \"$PERSIST_STATE_SEQUENCE\" \"$cwd\" \"$PERSIST_STATE_ENV_POLICY_FINGERPRINT\" \\\n\
                  \"$env_set\" \"$env_unset\" >\"$tmp\" || exit 1\n\
                chmod 600 \"$tmp\" || exit 1\n\
                mv -f \"$tmp\" \"$PERSIST_STATE_FILE\" || exit 1\n\
              else\n\
                cat >/dev/null\n\
              fi\n",
        )
        .expect("write state helper");
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))
            .expect("secure state helper");
        Some(Self {
            temp,
            home,
            runtime,
            holder,
            helper,
        })
    }

    fn start(&self, crash_point: Option<&str>) -> Child {
        let mut command = Command::new(env!("CARGO_BIN_EXE_persistd"));
        command
            .arg("foreground")
            .env("HOME", &self.home)
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("XDG_DATA_HOME", self.temp.path().join("data"))
            .env("XDG_STATE_HOME", self.temp.path().join("state"))
            .env("PERSIST_HOLDER_PATH", &self.holder)
            .env("PERSIST_TEST_HELPER_PATH", &self.helper)
            .env("STAGE7_API_TOKEN", "stage7-secret-must-not-persist")
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(point) = crash_point {
            command.env("PERSIST_TEST_CRASH_POINT", point);
        }
        command.spawn().expect("start daemon")
    }

    fn socket(&self) -> PathBuf {
        self.runtime.join("persistshell/persist.sock")
    }

    fn metadata(&self) -> MetadataStore {
        MetadataStore::open(&self.temp.path().join("data/persistshell/metadata.db"))
            .expect("open metadata")
    }
}

fn wait_for_socket(env: &TestEnv, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("check daemon") {
            let mut stderr = String::new();
            if let Some(output) = child.stderr.as_mut() {
                let _ = output.read_to_string(&mut stderr);
            }
            panic!("daemon exited with {status}: {stderr}");
        }
        if env.socket().exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for public socket");
}

fn wait_for_crash(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("wait crash") {
            if status.code() != Some(86) {
                let mut stderr = String::new();
                if let Some(output) = child.stderr.as_mut() {
                    let _ = output.read_to_string(&mut stderr);
                }
                panic!("unexpected daemon exit {status}: {stderr}");
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut stderr = String::new();
    if let Some(output) = child.stderr.as_mut() {
        let _ = output.read_to_string(&mut stderr);
    }
    panic!("daemon did not reach crash point: {stderr}");
}

fn stop_daemon(child: &mut Child) {
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().expect("stop daemon").is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    panic!("daemon did not stop");
}

fn connect(env: &TestEnv) -> ClientSocket {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut client = loop {
        match ClientSocket::connect(&env.socket()) {
            Ok(client) => break client,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("connect daemon: {error}"),
        }
    };
    client
        .send_hello(unsafe { libc::getuid() }, std::process::id())
        .expect("hello");
    client
}

fn create_session(client: &mut ClientSocket) -> u32 {
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::NewSession,
            flags: 0,
            request_id: 1,
            payload: Vec::new(),
        },
    )
    .expect("create session");
    let response = read_frame(client.stream()).expect("create response");
    decode_new_session_resp(&response.payload)
        .expect("decode create response")
        .session_id
}

fn list_sessions(client: &mut ClientSocket) -> persist_ipc::ListSessionsRespPayload {
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::ListSessions,
            flags: 0,
            request_id: 2,
            payload: Vec::new(),
        },
    )
    .expect("list sessions");
    let response = read_frame(client.stream()).expect("list response");
    decode_list_sessions_resp(&response.payload).expect("decode list")
}

fn attach(client: &mut ClientSocket, session_id: u32) -> bool {
    attach_payload(client, encode_attach(&AttachPayload { session_id }))
}

fn attach_with_context(
    client: &mut ClientSocket,
    session_id: u32,
    variables: &[(&str, &str)],
) -> bool {
    let context = ConnectionEnvironment::from_pairs(variables.iter().copied()).expect("context");
    attach_payload(
        client,
        encode_attach_with_context(&AttachPayload { session_id }, &context),
    )
}

fn attach_payload(client: &mut ClientSocket, payload: Vec<u8>) -> bool {
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Attach,
            flags: 0,
            request_id: 3,
            payload,
        },
    )
    .expect("attach session");
    for _ in 0..4 {
        let response = read_frame(client.stream()).expect("attach response");
        if response.msg_type == MessageType::AttachResp {
            return decode_attach_resp(&response.payload)
                .expect("decode attach")
                .ok;
        }
    }
    panic!("missing attach response");
}

fn send_stdin(client: &mut ClientSocket, payload: Vec<u8>) {
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Stdin,
            flags: 0,
            request_id: 4,
            payload,
        },
    )
    .expect("send stdin");
}

fn read_stdout_until(client: &mut ClientSocket, marker: &[u8]) -> bool {
    client
        .stream()
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    let mut output = Vec::new();
    while output.len() < 256 * 1024 {
        let frame = read_frame(client.stream()).expect("read output");
        if frame.msg_type == MessageType::Stdout {
            output.extend_from_slice(&frame.payload);
            if output.windows(marker.len()).any(|window| window == marker) {
                return true;
            }
        }
        if frame.msg_type == MessageType::SessionExited {
            return false;
        }
    }
    false
}

fn wait_for_closed(env: &TestEnv, session_id: u32, exit_code: i32, cwd: Option<&Path>) {
    let deadline = Instant::now() + LIFECYCLE_TIMEOUT;
    loop {
        let record = env
            .metadata()
            .get_session(session_id)
            .expect("read final metadata")
            .expect("Session metadata");
        if record.status == "closed" && record.exit_code == Some(exit_code) {
            if let Some(cwd) = cwd {
                assert_eq!(record.cwd.as_deref(), cwd.to_str());
            }
            return;
        }
        assert!(Instant::now() < deadline, "Session did not close");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_state_file(env: &TestEnv) -> PathBuf {
    let state_dir = env.runtime.join("persistshell/session-state");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(path) = std::fs::read_dir(&state_dir)
            .ok()
            .and_then(|mut entries| entries.next())
            .and_then(Result::ok)
            .map(|entry| entry.path())
        {
            return path;
        }
        assert!(Instant::now() < deadline, "state file did not appear");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_state_cwd(env: &TestEnv, cwd: &Path) {
    let expected = cwd.to_string_lossy();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let path = wait_for_state_file(env);
        if std::fs::read_to_string(path).is_ok_and(|state| state.contains(expected.as_ref())) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "final cwd was not committed before Ctrl-D"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn request_json(
    client: &mut ClientSocket,
    message_type: MessageType,
    payload: Vec<u8>,
) -> serde_json::Value {
    write_frame(
        client.stream(),
        &Frame {
            msg_type: message_type,
            flags: 0,
            request_id: 9,
            payload,
        },
    )
    .expect("request JSON");
    let response = read_frame(client.stream()).expect("JSON response");
    let json = decode_note_get_resp(&response.payload).expect("decode JSON response");
    serde_json::from_str(&json).expect("valid JSON response")
}

fn set_session_flag(client: &mut ClientSocket, message_type: MessageType, payload: Vec<u8>) {
    write_frame(
        client.stream(),
        &Frame {
            msg_type: message_type,
            flags: 0,
            request_id: 10,
            payload,
        },
    )
    .expect("set Session flag");
    let response = read_frame(client.stream()).expect("Session flag response");
    let result = decode_op_resp(&response.payload).expect("decode Session flag response");
    assert!(
        result.ok,
        "setting Session flag failed: {}",
        result.error_msg
    );
}

#[test]
fn create_crash_isolates_orphan_across_reconciliation_restart() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let mut first = env.start(Some("after_holder_create"));
    wait_for_socket(&env, &mut first);
    let mut client = connect(&env);
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::NewSession,
            flags: 0,
            request_id: 1,
            payload: Vec::new(),
        },
    )
    .expect("trigger create crash");
    wait_for_crash(&mut first);
    drop(client);
    assert!(env
        .metadata()
        .list_sessions()
        .expect("metadata list")
        .is_empty());

    let mut second = env.start(Some("after_reconcile"));
    wait_for_crash(&mut second);

    let mut third = env.start(None);
    wait_for_socket(&env, &mut third);
    let mut client = connect(&env);
    let sessions = list_sessions(&mut client);
    let orphan = sessions
        .sessions
        .iter()
        .find(|entry| entry.status == "orphan")
        .expect("isolated orphan");
    assert!(!attach(&mut client, orphan.session_id));
    let new_id = create_session(&mut client);
    assert!(new_id > orphan.session_id);
    let record = env
        .metadata()
        .get_session(new_id)
        .expect("read metadata")
        .expect("new metadata");
    assert_eq!(record.status, "running");
    assert!(record.holder_instance.is_some());
    assert!(record.holder_generation.is_some());
    drop(client);
    stop_daemon(&mut third);
}

#[test]
fn daemon_offline_exit_preserves_final_cwd() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let mut first = env.start(Some("after_metadata_commit"));
    wait_for_socket(&env, &mut first);
    let mut client = connect(&env);
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::NewSession,
            flags: 0,
            request_id: 1,
            payload: Vec::new(),
        },
    )
    .expect("trigger metadata commit crash");
    wait_for_crash(&mut first);
    drop(client);
    let session_id = env
        .metadata()
        .list_sessions()
        .expect("committed metadata")
        .pop()
        .expect("committed session")
        .session_id;
    let final_cwd = env.temp.path().join("offline-final");
    std::fs::create_dir(&final_cwd).expect("create offline final cwd");

    let mut second = env.start(None);
    wait_for_socket(&env, &mut second);
    let mut client = connect(&env);
    assert!(attach(&mut client, session_id));
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Stdin,
            flags: 0,
            request_id: 4,
            payload: format!(
                "stty -echo; cd '{}'; echo offline_exit_marker; sleep 1; exit 23\n",
                final_cwd.to_string_lossy()
            )
            .into_bytes(),
        },
    )
    .expect("start offline exit");
    std::thread::sleep(Duration::from_millis(75));
    unsafe { libc::kill(second.id() as i32, libc::SIGKILL) };
    second.wait().expect("wait killed daemon");
    drop(client);
    std::thread::sleep(Duration::from_millis(1200));

    let mut third = env.start(Some("after_reconcile"));
    wait_for_crash(&mut third);
    let closed = env
        .metadata()
        .get_session(session_id)
        .expect("read reconciled metadata")
        .expect("reconciled session");
    assert_eq!(closed.status, "closed");
    assert_eq!(closed.exit_code, Some(23));
    assert_eq!(closed.cwd.as_deref(), final_cwd.to_str());

    let mut fourth = env.start(None);
    wait_for_socket(&env, &mut fourth);
    let mut client = connect(&env);
    let sessions = list_sessions(&mut client);
    assert!(sessions.sessions.iter().any(|entry| {
        entry.session_id == session_id && entry.status == "closed" && entry.exit_code == Some(23)
    }));
    let log = std::fs::read(
        env.temp
            .path()
            .join(format!("data/persistshell/sessions/{session_id}.log")),
    )
    .expect("read Holder session log");
    assert!(log
        .windows(b"offline_exit_marker".len())
        .any(|window| { window == b"offline_exit_marker" }));
    drop(client);
    stop_daemon(&mut fourth);
}

#[test]
fn final_cwd_survives_crash_before_metadata() {
    assert_final_cwd_crash_window("after_exit_context_before_metadata", "running");
}

#[test]
fn restart_after_metadata_before_retire_is_idempotent() {
    assert_final_cwd_crash_window("after_exit_metadata_before_retire", "closed");
}

fn assert_final_cwd_crash_window(crash_point: &str, status_after_crash: &str) {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let final_cwd = env.temp.path().join("final-cwd");
    std::fs::create_dir(&final_cwd).expect("create final cwd");
    let mut first = env.start(Some(crash_point));
    wait_for_socket(&env, &mut first);
    let mut client = connect(&env);
    let session_id = create_session(&mut client);
    assert!(attach(&mut client, session_id));
    write_frame(
        client.stream(),
        &Frame {
            msg_type: MessageType::Stdin,
            flags: 0,
            request_id: 4,
            payload: format!("cd '{}'; exit 23\n", final_cwd.to_string_lossy()).into_bytes(),
        },
    )
    .expect("exit from final cwd");
    wait_for_crash(&mut first);
    drop(client);

    let record = env
        .metadata()
        .get_session(session_id)
        .expect("read crash-window metadata")
        .expect("Session metadata");
    assert_eq!(record.status, status_after_crash);
    let state_dir = env.runtime.join("persistshell/session-state");
    assert_eq!(
        std::fs::read_dir(&state_dir)
            .expect("state directory")
            .count(),
        1,
        "Holder must retain state before retire"
    );

    let mut second = env.start(None);
    wait_for_socket(&env, &mut second);
    let client = connect(&env);
    let closed = env
        .metadata()
        .get_session(session_id)
        .expect("read reconciled metadata")
        .expect("reconciled Session");
    assert_eq!(closed.status, "closed");
    assert_eq!(closed.exit_code, Some(23));
    assert_eq!(closed.cwd.as_deref(), final_cwd.to_str());
    assert_eq!(
        std::fs::read_dir(&state_dir)
            .expect("state directory after retire")
            .count(),
        0
    );
    drop(client);
    stop_daemon(&mut second);
}

#[test]
fn quick_cd_exit_restores_final_cwd() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let final_cwd = env.temp.path().join("quick-final");
    std::fs::create_dir(&final_cwd).expect("create final cwd");
    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut client = connect(&env);
    let session_id = create_session(&mut client);
    assert!(attach(&mut client, session_id));
    send_stdin(
        &mut client,
        format!("cd '{}'; exit 31\n", final_cwd.to_string_lossy()).into_bytes(),
    );
    wait_for_closed(&env, session_id, 31, Some(&final_cwd));

    let mut restored = connect(&env);
    assert!(attach(&mut restored, session_id));
    send_stdin(&mut restored, b"pwd; exit\n".to_vec());
    assert!(read_stdout_until(
        &mut restored,
        final_cwd.to_string_lossy().as_bytes()
    ));
    drop(restored);
    drop(client);
    stop_daemon(&mut daemon);
}

#[test]
fn running_attach_replays_output_after_client_disconnect() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut first = connect(&env);
    let session_id = create_session(&mut first);
    assert!(attach(&mut first, session_id));
    send_stdin(
        &mut first,
        b"sleep 0.2; printf '__RUNNING_%s__\\n' 'OFFLINE_OUTPUT'\n".to_vec(),
    );
    assert!(read_stdout_until(&mut first, b"sleep 0.2"));
    drop(first);
    std::thread::sleep(Duration::from_millis(400));

    let mut second = connect(&env);
    assert!(attach(&mut second, session_id));
    assert!(read_stdout_until(
        &mut second,
        b"__RUNNING_OFFLINE_OUTPUT__"
    ));
    send_stdin(&mut second, b"exit\n".to_vec());
    wait_for_closed(&env, session_id, 0, None);
    drop(second);
    stop_daemon(&mut daemon);
}

#[test]
fn closed_attach_restores_set_then_persists_unset() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut first = connect(&env);
    let session_id = create_session(&mut first);
    assert!(attach(&mut first, session_id));
    send_stdin(
        &mut first,
        b"export EDITOR=persist-stage6; exit 41\n".to_vec(),
    );
    wait_for_closed(&env, session_id, 41, None);

    let policy =
        persist_core::shell_state::EnvironmentPolicy::new(&["EDITOR".to_owned()], 128, 64 * 1024)
            .expect("policy");
    let first_record = env
        .metadata()
        .get_session(session_id)
        .expect("read first close")
        .expect("first record");
    let first_environment =
        persist_metadata::decode_environment(first_record.env_snapshot.as_deref(), &policy)
            .expect("decode first environment")
            .expect("first environment");
    assert_eq!(
        first_environment.env_set.get("EDITOR").map(String::as_str),
        Some("persist-stage6")
    );

    let mut second = connect(&env);
    assert!(attach(&mut second, session_id));
    send_stdin(
        &mut second,
        b"printf 'stage6-set=%s\\n' \"$EDITOR\"; unset EDITOR; exit 42\n".to_vec(),
    );
    assert!(read_stdout_until(&mut second, b"stage6-set=persist-stage6"));
    wait_for_closed(&env, session_id, 42, None);
    let second_record = env
        .metadata()
        .get_session(session_id)
        .expect("read second close")
        .expect("second record");
    let second_environment =
        persist_metadata::decode_environment(second_record.env_snapshot.as_deref(), &policy)
            .expect("decode second environment")
            .expect("second environment");
    assert!(second_environment.env_unset.contains("EDITOR"));

    let mut third = connect(&env);
    assert!(attach(&mut third, session_id));
    send_stdin(
        &mut third,
        b"printf 'stage6-unset=%s\\n' \"${EDITOR-unset}\"; exit 43\n".to_vec(),
    );
    assert!(read_stdout_until(&mut third, b"stage6-unset=unset"));
    wait_for_closed(&env, session_id, 43, None);

    drop(third);
    drop(second);
    drop(first);
    stop_daemon(&mut daemon);
}

#[test]
fn second_client_connection_context_overrides_restored_shell_context() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut first = connect(&env);
    let session_id = create_session(&mut first);
    assert!(attach_with_context(
        &mut first,
        session_id,
        &[("TERM", "stage7-client-a")]
    ));
    send_stdin(
        &mut first,
        b"export EDITOR=persist-stage6; exit 51\n".to_vec(),
    );
    wait_for_closed(&env, session_id, 51, None);

    let mut second = connect(&env);
    assert!(attach_with_context(
        &mut second,
        session_id,
        &[("TERM", "stage7-client-b")]
    ));
    send_stdin(
        &mut second,
        b"printf 'stage7-editor=%s term=%s\\n' \"$EDITOR\" \"$TERM\"; exit 52\n".to_vec(),
    );
    assert!(read_stdout_until(
        &mut second,
        b"stage7-editor=persist-stage6 term=stage7-client-b"
    ));
    wait_for_closed(&env, session_id, 52, None);
    drop(second);
    drop(first);
    stop_daemon(&mut daemon);
}

#[test]
fn inherited_sensitive_environment_does_not_persist() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut client = connect(&env);
    let session_id = create_session(&mut client);
    assert!(attach(&mut client, session_id));
    send_stdin(&mut client, b"exit 61\n".to_vec());
    wait_for_closed(&env, session_id, 61, None);

    for root in [
        env.temp.path().join("data"),
        env.temp.path().join("state"),
        env.runtime.join("persistshell"),
    ] {
        assert_tree_excludes(
            &root,
            &[b"STAGE7_API_TOKEN", b"stage7-secret-must-not-persist"],
        );
    }
    drop(client);
    stop_daemon(&mut daemon);
}

fn assert_tree_excludes(root: &Path, forbidden: &[&[u8]]) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            assert_tree_excludes(&path, forbidden);
        } else if path.is_file() {
            let bytes = std::fs::read(&path).expect("read leak-scan file");
            for value in forbidden {
                assert!(
                    !bytes.windows(value.len()).any(|window| window == *value),
                    "sensitive fixture persisted in {}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn ctrl_d_restores_final_cwd() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let final_cwd = env.temp.path().join("ctrl-d-final");
    std::fs::create_dir(&final_cwd).expect("create final cwd");
    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut client = connect(&env);
    let session_id = create_session(&mut client);
    assert!(attach(&mut client, session_id));
    send_stdin(
        &mut client,
        format!(
            "cd '{}'; printf '__CTRL_D_READY__\\n'\n",
            final_cwd.to_string_lossy()
        )
        .into_bytes(),
    );
    assert!(read_stdout_until(&mut client, b"__CTRL_D_READY__"));
    wait_for_state_cwd(&env, &final_cwd);
    send_stdin(&mut client, vec![0x15, 0x04]);
    wait_for_closed(&env, session_id, 0, Some(&final_cwd));
    drop(client);

    let mut restored = connect(&env);
    assert!(attach(&mut restored, session_id));
    assert!(read_stdout_until(&mut restored, b"__CTRL_D_READY__"));
    send_stdin(&mut restored, b"exit\n".to_vec());
    wait_for_closed(&env, session_id, 0, Some(&final_cwd));
    drop(restored);
    stop_daemon(&mut daemon);
}

#[test]
fn invalid_state_file_falls_back_without_blocking_exit() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    std::fs::write(env.home.join(".bashrc"), "trap ':' EXIT\n").expect("user EXIT trap");
    let final_cwd = env.temp.path().join("invalid-state-final");
    std::fs::create_dir(&final_cwd).expect("create final cwd");
    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut client = connect(&env);
    let session_id = create_session(&mut client);
    assert!(attach(&mut client, session_id));
    send_stdin(
        &mut client,
        format!(
            "cd '{}'; printf '__CORRUPT_READY__\\n'\n",
            final_cwd.to_string_lossy()
        )
        .into_bytes(),
    );
    assert!(read_stdout_until(&mut client, b"__CORRUPT_READY__"));
    let state_file = wait_for_state_file(&env);
    std::fs::write(&state_file, b"not-json").expect("corrupt state");
    std::fs::set_permissions(&state_file, std::fs::Permissions::from_mode(0o600))
        .expect("state mode");
    send_stdin(&mut client, b"exit 9\n".to_vec());
    wait_for_closed(&env, session_id, 9, None);
    assert!(!state_file.exists());
    drop(client);
    stop_daemon(&mut daemon);
}

#[test]
fn active_metadata_without_holder_becomes_inaccessible_lost_session() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let mut metadata = env.metadata();
    metadata
        .create_session(41, "missing-runtime", Some("/"), Some("/bin/sh"))
        .expect("create active metadata");
    drop(metadata);

    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut client = connect(&env);
    let sessions = list_sessions(&mut client);
    assert!(sessions
        .sessions
        .iter()
        .any(|entry| entry.session_id == 41 && entry.status == "lost"));
    assert!(!attach(&mut client, 41));
    let record = env
        .metadata()
        .get_session(41)
        .expect("read lost metadata")
        .expect("lost session");
    assert_eq!(record.status, "lost");
    assert!(record.holder_instance.is_some());
    assert!(record.holder_generation.is_some());
    drop(client);
    stop_daemon(&mut daemon);
}

#[test]
fn holder_crash_marks_live_session_lost_without_stopping_daemon() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut client = connect(&env);
    let session_id = create_session(&mut client);
    let holder_pid = std::fs::read_to_string(env.runtime.join("persistshell/holder.pid"))
        .expect("holder pid")
        .trim()
        .parse::<i32>()
        .expect("parse holder pid");
    unsafe { libc::kill(holder_pid, libc::SIGKILL) };

    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let status = env
            .metadata()
            .get_session(session_id)
            .expect("read metadata")
            .expect("session metadata")
            .status;
        if status == "lost" {
            break;
        }
        assert!(Instant::now() < deadline, "Holder crash was not reconciled");
        assert!(daemon.try_wait().expect("check daemon").is_none());
        std::thread::sleep(Duration::from_millis(50));
    }
    let sessions = list_sessions(&mut client);
    assert!(sessions
        .sessions
        .iter()
        .any(|entry| entry.session_id == session_id && entry.status == "lost"));
    let metrics = request_json(&mut client, MessageType::Metrics, Vec::new());
    assert_eq!(metrics["holder"]["connected"], false);
    assert!(metrics["holder"]["pid"].as_u64().is_some());
    assert_eq!(
        metrics["holder"]["instance"].as_str().map(str::len),
        Some(32)
    );
    assert_eq!(metrics["sessions"]["lost"], 1);
    assert!(!attach(&mut client, session_id));
    drop(client);
    stop_daemon(&mut daemon);
}

#[test]
fn holder_log_failure_is_visible_in_snapshot_and_metrics() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    let logs = env.temp.path().join("data/persistshell/sessions");
    let target = env.temp.path().join("must-not-be-written.log");
    std::fs::create_dir_all(&logs).expect("log directory");
    symlink(&target, logs.join("1.log")).expect("log symlink");

    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut client = connect(&env);
    let session_id = create_session(&mut client);
    assert_eq!(session_id, 1);
    let initial_generation = env
        .metadata()
        .get_session(session_id)
        .expect("initial metadata")
        .expect("initial Session")
        .holder_generation
        .expect("initial Holder generation");

    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let metrics = request_json(&mut client, MessageType::Metrics, Vec::new());
        if metrics["sessions"]["log_degraded"] == 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "log degradation was not observed"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let snapshot = request_json(
        &mut client,
        MessageType::SessionSnapshot,
        encode_detach(&DetachPayload { session_id }),
    );
    assert_eq!(snapshot["output_log_state"], "degraded");
    let degraded_generation = env
        .metadata()
        .get_session(session_id)
        .expect("degraded metadata")
        .expect("degraded Session")
        .holder_generation
        .expect("degraded Holder generation");
    assert!(degraded_generation > initial_generation);
    assert!(!target.exists(), "Holder followed a Session log symlink");
    drop(client);
    stop_daemon(&mut daemon);
}

#[test]
fn holder_idle_gc_respects_pinned_and_locked_sessions() {
    let Some(env) = TestEnv::new() else {
        return;
    };
    std::fs::write(
        env.temp.path().join("config/persistshell/config.toml"),
        "[daemon]\ngc_idle_timeout = \"500ms\"\ngc_interval = \"100ms\"\n\
         [logging]\nsession_log = true\n",
    )
    .expect("write GC config");
    let mut daemon = env.start(None);
    wait_for_socket(&env, &mut daemon);
    let mut client = connect(&env);
    let pinned = create_session(&mut client);
    let locked = create_session(&mut client);
    let collectable = create_session(&mut client);
    set_session_flag(
        &mut client,
        MessageType::PinSet,
        encode_pin(&PinPayload {
            session_id: pinned,
            pinned: true,
        }),
    );
    set_session_flag(
        &mut client,
        MessageType::LockSet,
        encode_lock(&LockPayload {
            session_id: locked,
            locked: true,
        }),
    );

    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let records = env.metadata().list_sessions().expect("list GC metadata");
        let status = |id| {
            records
                .iter()
                .find(|record| record.session_id == id)
                .map(|record| record.status.as_str())
        };
        if status(collectable) == Some("closed") {
            assert_eq!(status(pinned), Some("running"));
            assert_eq!(status(locked), Some("running"));
            break;
        }
        assert!(Instant::now() < deadline, "Idle GC did not close Session");
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(client);
    stop_daemon(&mut daemon);
}
