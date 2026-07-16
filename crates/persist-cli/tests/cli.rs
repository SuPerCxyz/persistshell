use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

fn persist_cmd() -> Command {
    let root = test_root();
    let mut command = Command::new(env!("CARGO_BIN_EXE_persist"));
    command
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"));
    command
}

fn test_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("persist_cli_test_{}", std::process::id()));
        let runtime = root.join("runtime");
        fs::create_dir_all(&runtime).expect("create CLI test runtime directory");
        root
    })
}

fn run(args: &[&str]) -> (bool, String, String) {
    let output = persist_cmd().args(args).output().expect("run persist");
    (
        output.status.success(),
        String::from_utf8(output.stdout).expect("stdout utf8"),
        String::from_utf8(output.stderr).expect("stderr utf8"),
    )
}

#[test]
fn version_command_prints_binary_name() {
    let (ok, stdout, _) = run(&["--version"]);
    assert!(ok);
    assert!(stdout.starts_with("persist "));
}

#[test]
fn help_command_prints_usage() {
    let (ok, stdout, _) = run(&["help"]);
    assert!(ok);
    assert!(stdout.contains("Usage"));
    assert!(stdout.contains("top"));
}

#[test]
fn top_requires_interactive_terminal() {
    let (ok, _, stderr) = run(&["top"]);
    assert!(!ok);
    assert!(stderr.contains("E_INVALID_ARGUMENT"));
    assert!(stderr.contains("interactive terminal"));
}

#[test]
fn unknown_command_shows_error_code() {
    let (ok, _, stderr) = run(&["wat"]);
    assert!(!ok);
    assert!(stderr.contains("E_INVALID_ARGUMENT"));
    assert!(stderr.contains("unknown persist command"));
}

#[test]
fn detach_requires_session_id() {
    let (ok, _, stderr) = run(&["detach"]);
    assert!(!ok);
    assert!(stderr.contains("E_INVALID_ARGUMENT"));
}

#[test]
fn note_requires_session_id() {
    let (ok, _, stderr) = run(&["note"]);
    assert!(!ok);
    assert!(stderr.contains("E_INVALID_ARGUMENT"));
    assert!(stderr.contains("usage: persist note"));
}

#[test]
fn note_without_daemon_shows_connection_error() {
    let (ok, _, stderr) = run(&["note", "1", "hello"]);
    assert!(!ok);
    assert!(stderr.contains("E_IO"));
}

#[test]
fn tag_requires_session_id() {
    let (ok, _, stderr) = run(&["tag"]);
    assert!(!ok);
    assert!(stderr.contains("E_INVALID_ARGUMENT"));
}

#[test]
fn tag_add_requires_tag_value() {
    let (ok, _, stderr) = run(&["tag", "1", "add"]);
    assert!(!ok);
    assert!(stderr.contains("E_INVALID_ARGUMENT"));
}

#[test]
fn tag_unknown_action_shows_error() {
    let (ok, _, stderr) = run(&["tag", "1", "wat"]);
    assert!(!ok);
    assert!(stderr.contains("E_INVALID_ARGUMENT"));
}

#[test]
fn rename_requires_session_id() {
    let (ok, _, stderr) = run(&["rename"]);
    assert!(!ok);
    assert!(stderr.contains("E_INVALID_ARGUMENT"));
}

#[test]
fn rename_requires_name() {
    let (ok, _, stderr) = run(&["rename", "1"]);
    assert!(!ok);
    assert!(stderr.contains("E_INVALID_ARGUMENT"));
}

#[test]
fn daemon_required_commands_show_connection_error() {
    let cases: &[&[&str]] = &[
        &["new"],
        &["ls"],
        &["close", "1"],
        &["kill", "1"],
        &["rename", "1", "foo"],
        &["detach", "1"],
        &["note", "1", "text"],
        &["tag", "1", "list"],
    ];
    for args in cases {
        let (ok, _, stderr) = run(args);
        assert!(!ok, "expected failure for: {:?}", args);
        assert!(
            stderr.contains("E_IO"),
            "expected E_IO for {:?}, got: {}",
            args,
            stderr
        );
    }
}

#[test]
fn ls_accepts_tag_flag() {
    let (ok, _, stderr) = run(&["ls", "--tag", "work"]);
    assert!(!ok);
    assert!(stderr.contains("E_IO"));
}

#[test]
fn log_export_requires_session_id() {
    let (ok, _, stderr) = run(&["log", "export"]);
    assert!(!ok);
    assert!(stderr.contains("E_INVALID_ARGUMENT"));
    assert!(stderr.contains("usage: persist log export"));
}

#[test]
fn log_export_accepts_output_flag() {
    let (ok, _, stderr) = run(&["log", "export", "1", "--output", "/tmp/test.log"]);
    // Config may not exist, but command parsing succeeds.
    // The actual error will be from log file not found.
    assert!(!ok);
    assert!(stderr.contains("no log file for session 1"));
}

#[test]
fn ls_accepts_short_tag_flag() {
    let (ok, _, stderr) = run(&["ls", "-t", "work"]);
    assert!(!ok);
    assert!(stderr.contains("E_IO"));
}
