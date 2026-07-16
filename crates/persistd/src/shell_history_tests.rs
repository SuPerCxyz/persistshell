use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use persist_pty::PtyEngine;

use crate::shell_history::prepare;

#[test]
fn unsupported_shell_does_not_install_hook() {
    let root = temp_root("unsupported");
    let helper = create_helper(&root);
    assert!(prepare("/bin/sh", 1, &root, &helper).unwrap().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn bash_hook_is_private_and_composes_prompt_command() {
    let root = temp_root("bash-unit");
    let helper = create_helper(&root);
    let launch = prepare("/bin/bash", 8, &root, &helper).unwrap().unwrap();
    let rcfile = Path::new(&launch.arguments[1]);
    let content = fs::read_to_string(rcfile).unwrap();
    assert!(content.contains("source \"$HOME/.bashrc\""));
    assert!(content.contains("PROMPT_COMMAND+=(__persist_history_capture)"));
    assert_eq!(
        fs::metadata(rcfile).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fish_hook_checks_native_history_before_writing() {
    let root = temp_root("fish-unit");
    let helper = create_helper(&root);
    let launch = prepare("/usr/bin/fish", 3, &root, &helper)
        .unwrap()
        .unwrap();
    assert!(launch.arguments[1].contains("not functions -q fish_should_add_to_history"));
    assert_eq!(
        fs::read_to_string(root.join(".hooks/3/status")).unwrap(),
        "enabled\n"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn bash_preserves_user_config_hook_and_history_filter() {
    let root = temp_root("bash-live");
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(
        home.join(".bashrc"),
        "export USER_CONFIG_MARKER=bash-loaded\nprintf r >\"$PERSIST_READY_MARKER\"\nHISTCONTROL=ignorespace\nPROMPT_COMMAND='printf x >>\"$PERSIST_PROMPT_MARKER\"'\n",
    )
    .unwrap();
    assert_shell_behavior("/bin/bash", "bash", &root, &home);
}

#[test]
fn zsh_preserves_user_config_hook_and_history_filter() {
    let root = temp_root("zsh-live");
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(
        home.join(".zshrc"),
        "export USER_CONFIG_MARKER=zsh-loaded\nprintf r >\"$PERSIST_READY_MARKER\"\nuser_precmd() { print -rn x >>\"$PERSIST_PROMPT_MARKER\"; }\nprecmd_functions+=(user_precmd)\n",
    )
    .unwrap();
    assert_shell_behavior("/usr/bin/zsh", "zsh", &root, &home);
}

#[test]
fn fish_preserves_user_config_hook_and_history_filter() {
    let root = temp_root("fish-live");
    let home = root.join("home");
    let fish_config = root.join("config/fish");
    fs::create_dir_all(&fish_config).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(
        fish_config.join("config.fish"),
        "set -gx USER_CONFIG_MARKER fish-loaded\nprintf r >\"$PERSIST_READY_MARKER\"\nfunction user_postexec --on-event fish_postexec\n    printf x >>\"$PERSIST_PROMPT_MARKER\"\nend\n",
    )
    .unwrap();
    assert_shell_behavior("/usr/bin/fish", "fish", &root, &home);
}

#[test]
fn zsh_custom_history_filter_disables_capture_without_replacing_hooks() {
    assert_custom_filter_degrades("/usr/bin/zsh", "zsh");
}

#[test]
fn fish_custom_history_filter_disables_capture_without_replacing_hooks() {
    assert_custom_filter_degrades("/usr/bin/fish", "fish");
}

fn assert_shell_behavior(shell: &str, shell_name: &str, root: &Path, home: &Path) {
    if !Path::new(shell).is_file() {
        return;
    }
    let helper = create_helper(root);
    let capture = root.join("captured");
    let prompt_marker = root.join("prompt-marker");
    let ready_marker = root.join("ready-marker");
    let launch = prepare(shell, 17, root, &helper).unwrap().unwrap();
    let mut environment = launch.environment;
    environment.extend([
        ("HOME".into(), home.to_string_lossy().into_owned()),
        (
            "PERSIST_CAPTURE".into(),
            capture.to_string_lossy().into_owned(),
        ),
        (
            "PERSIST_PROMPT_MARKER".into(),
            prompt_marker.to_string_lossy().into_owned(),
        ),
        (
            "PERSIST_READY_MARKER".into(),
            ready_marker.to_string_lossy().into_owned(),
        ),
        (
            "XDG_CONFIG_HOME".into(),
            root.join("config").to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".into(),
            root.join("data").to_string_lossy().into_owned(),
        ),
    ]);
    let histfile = root.join("native-history");
    let mut session = PtyEngine::new()
        .open_session_with_context_and_args(
            shell,
            Some(&histfile.to_string_lossy()),
            None,
            &environment,
            &launch.arguments,
        )
        .unwrap();
    wait_for_file(&ready_marker, "r");
    writeln!(session, "echo $USER_CONFIG_MARKER").unwrap();
    if shell_name == "bash" {
        writeln!(session, " echo secret").unwrap();
    }
    writeln!(session, "echo visible-{shell_name}").unwrap();

    wait_for_file(&capture, &format!("echo visible-{shell_name}"));
    let captured = fs::read_to_string(&capture).unwrap_or_default();
    assert!(
        captured.contains("echo $USER_CONFIG_MARKER"),
        "captured={captured:?}"
    );
    assert!(captured.contains(&format!("echo visible-{shell_name}")));
    assert!(!captured.contains("echo secret"), "captured={captured:?}");
    assert!(fs::metadata(prompt_marker).is_ok());
    writeln!(session, "exit").unwrap();
    let _ = fs::remove_dir_all(root);
}

fn assert_custom_filter_degrades(shell: &str, shell_name: &str) {
    if !Path::new(shell).is_file() {
        return;
    }
    let root = temp_root(&format!("{shell_name}-filter"));
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    if shell_name == "zsh" {
        fs::write(
            home.join(".zshrc"),
            "zshaddhistory() { [[ $1 != *secret* ]]; }\nuser_precmd() { print -rn x >>\"$PERSIST_PROMPT_MARKER\"; }\nprecmd_functions+=(user_precmd)\n",
        )
        .unwrap();
    } else {
        let config = root.join("config/fish");
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join("config.fish"),
            "function fish_should_add_to_history\n    not string match -q '*secret*' -- $argv[1]\nend\nfunction user_postexec --on-event fish_postexec\n    printf x >>\"$PERSIST_PROMPT_MARKER\"\nend\n",
        )
        .unwrap();
    }
    let helper = create_helper(&root);
    let capture = root.join("captured");
    let prompt = root.join("prompt-marker");
    let launch = prepare(shell, 18, &root, &helper).unwrap().unwrap();
    let mut environment = launch.environment;
    environment.extend([
        ("HOME".into(), home.to_string_lossy().into_owned()),
        (
            "PERSIST_CAPTURE".into(),
            capture.to_string_lossy().into_owned(),
        ),
        (
            "PERSIST_PROMPT_MARKER".into(),
            prompt.to_string_lossy().into_owned(),
        ),
        (
            "XDG_CONFIG_HOME".into(),
            root.join("config").to_string_lossy().into_owned(),
        ),
    ]);
    let mut session = PtyEngine::new()
        .open_session_with_context_and_args(shell, None, None, &environment, &launch.arguments)
        .unwrap();
    std::thread::sleep(Duration::from_millis(250));
    writeln!(session, "echo visible").unwrap();
    wait_for_file(&prompt, "x");
    assert!(!capture.exists() || fs::read(&capture).unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(root.join(".hooks/18/status")).unwrap(),
        "filtered\n"
    );
    assert!(fs::metadata(prompt).is_ok());
    writeln!(session, "exit").unwrap();
    let _ = fs::remove_dir_all(root);
}

fn wait_for_file(path: &Path, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if fs::read_to_string(path).is_ok_and(|content| content.contains(expected)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("history helper did not capture {expected}");
}

fn temp_root(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("persistshell-hooks-{name}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn create_helper(root: &Path) -> PathBuf {
    let helper = root.join("persist");
    fs::write(
        &helper,
        b"#!/bin/sh\ncat >>\"$PERSIST_CAPTURE\"\nprintf '\\036' >>\"$PERSIST_CAPTURE\"\n",
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
    helper
}
