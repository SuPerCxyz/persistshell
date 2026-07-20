use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use persist_pty::{PtyEngine, PtySession};

use crate::shell_history::ShellLaunch;

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
    assert!(content.contains("PROMPT_COMMAND+=(__persist_state_commit)"));
    assert!(content.contains("trap '__persist_state_exit' EXIT"));
    assert!(launch
        .environment
        .iter()
        .any(|(key, value)| key == "PERSIST_STATE_SESSION_ID" && value == "8"));
    assert!(launch
        .environment
        .iter()
        .any(|(key, value)| key == "PERSIST_STATE_HELPER" && value == &helper.to_string_lossy()));
    assert!(launch
        .environment
        .iter()
        .any(|(key, value)| key == "PERSIST_STATE_ENV_MAX_VARIABLES" && value == "128"));
    assert_eq!(
        fs::metadata(rcfile).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn shell_launch_carries_validated_environment_policy_hint() {
    let root = temp_root("environment-policy");
    let helper = create_helper(&root);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let identity = persist_core::shell_state::create_identity(&root, 9).unwrap();
    let config = persist_core::RecoveryEnvironmentConfig {
        include: vec!["EDITOR".into(), "MY_PROJECT_*".into()],
        max_variables: 32,
        max_bytes: persist_core::ByteSize::from_bytes(16 * 1024),
    };
    let launch = crate::shell_history::prepare_with_policy(
        "/bin/bash",
        9,
        &root,
        &helper,
        &identity,
        &config,
    )
    .unwrap()
    .unwrap();
    let values = launch
        .environment
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    let policy =
        persist_core::shell_state::EnvironmentPolicy::new(&config.include, 32, 16 * 1024).unwrap();

    assert_eq!(values["PERSIST_STATE_ENV_INCLUDE"], "EDITOR,MY_PROJECT_*");
    assert_eq!(values["PERSIST_STATE_ENV_MAX_VARIABLES"], "32");
    assert_eq!(values["PERSIST_STATE_ENV_MAX_BYTES"], "16384");
    assert_eq!(
        values["PERSIST_STATE_ENV_POLICY_FINGERPRINT"],
        policy.fingerprint()
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
    assert!(launch.arguments[1].contains("--on-event fish_postexec"));
    assert!(launch.arguments[1].contains("--on-event fish_exit"));
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
fn bash_preserves_existing_exit_trap_and_ignores_subshell_cwd() {
    let root = temp_root("bash-exit");
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(
        home.join(".bashrc"),
        "trap 'printf preserved >\"$PERSIST_EXIT_MARKER\"' EXIT\n",
    )
    .unwrap();
    let helper = create_helper(&root);
    let launch = prepare("/bin/bash", 19, &root, &helper).unwrap().unwrap();
    let state_capture = root.join("state-capture");
    let exit_marker = root.join("exit-marker");
    let trap_definition = root.join("trap-definition");
    let mut command = std::process::Command::new("/bin/bash");
    command
        .args(&launch.arguments)
        .arg("-c")
        .arg(format!(
            "__persist_state_commit; (cd /tmp; __persist_state_commit); \
             cd /; __persist_state_commit; trap -p EXIT >{}; exit 23",
            trap_definition.to_string_lossy()
        ))
        .env_clear();
    for (key, value) in launch.environment {
        command.env(key, value);
    }
    let status = command
        .env("HOME", &home)
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("PERSIST_STATE_CAPTURE", &state_capture)
        .env("PERSIST_EXIT_MARKER", &exit_marker)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(23));
    assert_eq!(fs::read_to_string(exit_marker).unwrap(), "preserved");
    let definition = fs::read_to_string(trap_definition).unwrap();
    assert!(definition.contains("printf preserved"));
    assert_eq!(
        fs::read_to_string(root.join(".hooks/19/state-status")).unwrap(),
        "exit-conflict\n"
    );
    let states = fs::read_to_string(state_capture).unwrap();
    assert!(states.contains("/\u{1e}"));
    assert!(!states.contains("/tmp\u{1e}"));
    let _ = fs::remove_dir_all(root);
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
    let mut arguments = launch.arguments;
    let mut environment = launch.environment;
    if shell_name == "zsh" {
        arguments.insert(0, "-d".into());
        set_environment(&mut environment, "PERSIST_ORIGINAL_ZDOTDIR", home);
        set_environment(&mut environment, "PERSIST_ORIGINAL_ZDOTDIR_SET", "1");
    }
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
            &arguments,
        )
        .unwrap();
    wait_for_file_with_pty(&mut session, &ready_marker, "r", Duration::from_secs(30));
    writeln!(session, "echo $USER_CONFIG_MARKER").unwrap();
    if shell_name == "bash" {
        writeln!(session, " echo secret").unwrap();
    }
    writeln!(session, "echo visible-{shell_name}").unwrap();

    wait_for_file(
        &capture,
        &format!("echo visible-{shell_name}"),
        Duration::from_secs(10),
    );
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

fn set_environment(environment: &mut [(String, String)], name: &str, value: impl AsRef<Path>) {
    let value = value.as_ref().to_string_lossy().into_owned();
    if let Some((_, current)) = environment.iter_mut().find(|(key, _)| key == name) {
        *current = value;
    }
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
    wait_for_file(&prompt, "x", Duration::from_secs(10));
    assert!(!capture.exists() || fs::read(&capture).unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(root.join(".hooks/18/status")).unwrap(),
        "filtered\n"
    );
    assert!(fs::metadata(prompt).is_ok());
    writeln!(session, "exit").unwrap();
    let _ = fs::remove_dir_all(root);
}

fn wait_for_file(path: &Path, expected: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if fs::read_to_string(path).is_ok_and(|content| content.contains(expected)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("history helper did not capture {expected}");
}

fn wait_for_file_with_pty(
    session: &mut PtySession,
    path: &Path,
    expected: &str,
    timeout: Duration,
) {
    const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;
    let deadline = Instant::now() + timeout;
    let mut buffer = [0_u8; 4096];
    let mut output = Vec::new();
    while Instant::now() < deadline {
        while session.poll_output(Duration::ZERO).unwrap_or(false) {
            let read = session.read_output(&mut buffer).unwrap_or(0);
            if read == 0 {
                break;
            }
            let remaining = MAX_DIAGNOSTIC_BYTES.saturating_sub(output.len());
            output.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        if fs::read_to_string(path).is_ok_and(|content| content.contains(expected)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "shell did not become ready; output={:?}",
        String::from_utf8_lossy(&output)
    );
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
        b"#!/bin/sh\nif [ \"$1\" = __history-append ]; then\n\
          cat >>\"$PERSIST_CAPTURE\"\n\
          printf '\\036' >>\"$PERSIST_CAPTURE\"\n\
          elif [ -n \"$PERSIST_STATE_CAPTURE\" ]; then\n\
          cat >>\"$PERSIST_STATE_CAPTURE\"\n\
          printf '\\036' >>\"$PERSIST_STATE_CAPTURE\"\n\
          else\n\
          cat >/dev/null\n\
          fi\n",
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
    helper
}

fn prepare(
    shell: &str,
    session_id: u32,
    history_dir: &Path,
    helper: &Path,
) -> persist_core::Result<Option<ShellLaunch>> {
    fs::set_permissions(history_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let identity = persist_core::shell_state::create_identity(history_dir, session_id)?;
    crate::shell_history::prepare(shell, session_id, history_dir, helper, &identity)
}
