use std::io::Cursor;
use std::path::PathBuf;

use persist_core::{append_command, command_history_path};
use persist_ipc::{ListSessionsRespPayload, SessionEntry};

use crate::session_browser::browse_with;

#[test]
fn history_returns_to_menu_then_attach_and_list() {
    let data_dir = temp_data_dir("flow");
    let history = command_history_path(&data_dir, 1);
    append_command(&history, "bash", b"echo older").unwrap();
    append_command(&history, "bash", b"echo newest").unwrap();
    let mut input = Cursor::new(b"h\nb\na\nb\nq\n");
    let mut output = Vec::new();
    let mut attached = Vec::new();

    browse_with(
        &data_dir,
        Some(1),
        None,
        &mut input,
        &mut output,
        |_| Ok(session_list()),
        |session_id| {
            attached.push(session_id);
            Ok(())
        },
    )
    .unwrap();

    let text = String::from_utf8(output).unwrap();
    assert!(text.find("echo newest") < text.find("echo older"));
    assert!(text.contains("Session ID（q 退出）"));
    assert_eq!(attached, vec![1]);
    let _ = std::fs::remove_dir_all(data_dir.parent().unwrap());
}

#[test]
fn history_next_page_shows_older_records() {
    let data_dir = temp_data_dir("pagination");
    let history = command_history_path(&data_dir, 1);
    for number in 1..=51 {
        append_command(&history, "zsh", format!("echo {number}").as_bytes()).unwrap();
    }
    let mut input = Cursor::new(b"h\nn\nb\nq\n");
    let mut output = Vec::new();

    browse_with(
        &data_dir,
        Some(1),
        None,
        &mut input,
        &mut output,
        |_| Ok(session_list()),
        |_| Ok(()),
    )
    .unwrap();

    let text = String::from_utf8(output).unwrap();
    assert!(text.find("echo 51") < text.find("echo 1"));
    let _ = std::fs::remove_dir_all(data_dir.parent().unwrap());
}

#[test]
fn list_rejects_unknown_id_without_leaving_browser() {
    let data_dir = temp_data_dir("unknown");
    let mut input = Cursor::new(b"99\nq\n");
    let mut output = Vec::new();

    browse_with(
        &data_dir,
        None,
        None,
        &mut input,
        &mut output,
        |_| Ok(session_list()),
        |_| Ok(()),
    )
    .unwrap();

    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Session 99 不存在"));
    assert!(text.matches("Session ID（q 退出）").count() >= 2);
    let _ = std::fs::remove_dir_all(data_dir.parent().unwrap());
}

#[test]
fn filtered_shell_history_reports_safe_degradation() {
    let data_dir = temp_data_dir("filtered");
    let status = data_dir.join("history/.hooks/1/status");
    std::fs::create_dir_all(status.parent().unwrap()).unwrap();
    std::fs::write(status, b"filtered\n").unwrap();
    let mut input = Cursor::new(b"h\nb\nq\n");
    let mut output = Vec::new();

    browse_with(
        &data_dir,
        Some(1),
        None,
        &mut input,
        &mut output,
        |_| Ok(session_list()),
        |_| Ok(()),
    )
    .unwrap();

    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("检测到自定义 Shell history 过滤器"));
    let _ = std::fs::remove_dir_all(data_dir.parent().unwrap());
}

fn session_list() -> ListSessionsRespPayload {
    ListSessionsRespPayload {
        sessions: vec![SessionEntry {
            session_id: 1,
            name: "bash@work".into(),
            status: "running".into(),
            exit_code: None,
            closed_at: None,
            has_note: false,
            has_tags: false,
            is_pinned: false,
            is_locked: false,
            idle: "1s".into(),
            foreground_pid: None,
            foreground_name: String::new(),
            foreground_cmd: String::new(),
        }],
    }
}

fn temp_data_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "persistshell-browser-{name}-{}",
        std::process::id()
    ));
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    data_dir
}
