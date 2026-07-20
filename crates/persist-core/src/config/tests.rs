use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

#[test]
fn derives_xdg_paths_from_home() {
    let paths = test_paths("/tmp/runtime");

    assert_eq!(
        paths.config_dir,
        PathBuf::from("/home/alice/.config/persistshell")
    );
    assert_eq!(
        paths.socket_path,
        PathBuf::from("/tmp/runtime/persistshell/persist.sock")
    );
    assert_eq!(
        paths.holder_socket_path,
        PathBuf::from("/tmp/runtime/persistshell/holder.sock")
    );
}

#[test]
fn default_config_has_safe_values() {
    let config = Config::default_with_paths(test_paths("/tmp/runtime"));

    assert!(config.daemon.auto_start);
    assert!(config.session.new_session_on_ssh);
    assert!(config.ring_buffer.replay_on_attach);
    assert_eq!(config.internal_log.level.to_string(), "info");
    assert_eq!(
        config.internal_log.client_log,
        PathBuf::from("/home/alice/.local/state/persistshell/client.log")
    );
    assert!(!config.security.allow_root_attach_others);
    assert!(!config.security.enable_input_recording);
    assert_eq!(config.ssh.bypass_env, "PERSIST_DISABLE");
    assert!(config.recovery.environment.include.is_empty());
    assert_eq!(config.recovery.environment.max_variables, 128);
    assert_eq!(config.recovery.environment.max_bytes.to_string(), "64KB");
}

#[test]
fn recovery_environment_config_loads_safe_limits_and_include_rules() {
    let dir = TestDir::new("recovery-environment");
    let user_config = dir.path.join("user.toml");
    fs::write(
        &user_config,
        r#"
[recovery.environment]
include = ["EDITOR", "MY_PROJECT_*"]
max_variables = 32
max_bytes = "16KiB"
"#,
    )
    .expect("write user config");

    let options = ConfigLoadOptions::from_paths(
        test_paths("/tmp/runtime"),
        dir.path.join("missing-system.toml"),
        user_config,
    );
    let config = load_config(&options).expect("load config");

    assert_eq!(
        config.recovery.environment.include,
        ["EDITOR", "MY_PROJECT_*"]
    );
    assert_eq!(config.recovery.environment.max_variables, 32);
    assert_eq!(config.recovery.environment.max_bytes.to_string(), "16KB");
}

#[test]
fn recovery_environment_config_rejects_unsafe_rules_and_expanded_limits() {
    for (name, body) in [
        ("secret", "include = [\"API_TOKEN\"]"),
        ("glob", "include = [\"MY_*_TOKEN\"]"),
        ("variables", "max_variables = 129"),
        ("exact-count", "include = [\"EDITOR\"]\nmax_variables = 1"),
        ("bytes", "max_bytes = \"65KiB\""),
    ] {
        let dir = TestDir::new(name);
        let user_config = dir.path.join("user.toml");
        fs::write(&user_config, format!("[recovery.environment]\n{body}\n"))
            .expect("write user config");
        let options = ConfigLoadOptions::from_paths(
            test_paths("/tmp/runtime"),
            dir.path.join("missing-system.toml"),
            user_config,
        );

        assert!(load_config(&options).is_err(), "rule {name} must fail");
    }
}

#[test]
fn loads_system_then_user_config() {
    let dir = TestDir::new("merge");
    let system_config = dir.path.join("system.toml");
    let user_config = dir.path.join("user.toml");

    fs::write(
        &system_config,
        r#"
[daemon]
auto_start = false
idle_exit_after = "5m"

[ring_buffer]
default_size = "4MB"
max_size = "64MB"
"#,
    )
    .expect("write system config");
    fs::write(
        &user_config,
        r#"
[daemon]
idle_exit = false

[ring_buffer]
default_size = "16MB"

[runtime]
socket_dir = "/run/user/1234/custom"

[internal_log]
level = "debug"
client_log = "/tmp/persistshell-client.log"
max_file_size = "10MB"
"#,
    )
    .expect("write user config");

    let options =
        ConfigLoadOptions::from_paths(test_paths("/tmp/runtime"), system_config, user_config);
    let config = load_config(&options).expect("load config");

    assert!(!config.daemon.auto_start);
    assert!(!config.daemon.idle_exit);
    assert_eq!(config.daemon.idle_exit_after.to_string(), "5m");
    assert_eq!(config.ring_buffer.default_size.to_string(), "16MB");
    assert_eq!(config.ring_buffer.max_size.to_string(), "64MB");
    assert_eq!(
        config.runtime.socket_dir,
        PathBuf::from("/run/user/1234/custom")
    );
    assert_eq!(
        config.paths.socket_path,
        PathBuf::from("/run/user/1234/custom/persist.sock")
    );
    assert_eq!(
        config.paths.holder_socket_path,
        PathBuf::from("/run/user/1234/custom/holder.sock")
    );
    assert_eq!(config.internal_log.level.to_string(), "debug");
    assert_eq!(
        config.internal_log.client_log,
        PathBuf::from("/tmp/persistshell-client.log")
    );
    assert_eq!(config.internal_log.max_file_size.to_string(), "10MB");
}

#[test]
fn rejects_invalid_ring_buffer_limits() {
    let dir = TestDir::new("invalid-ring-buffer");
    let user_config = dir.path.join("user.toml");
    fs::write(
        &user_config,
        r#"
[ring_buffer]
default_size = "256MB"
max_size = "128MB"
"#,
    )
    .expect("write user config");

    let options = ConfigLoadOptions::from_paths(
        test_paths("/tmp/runtime"),
        dir.path.join("missing-system.toml"),
        user_config,
    );
    let error = load_config(&options).expect_err("invalid config");

    assert!(matches!(error, PersistError::ConfigValidation { .. }));
    assert!(error.to_string().contains("default_size"));
}

#[test]
fn rejects_parse_errors_with_path_context() {
    let dir = TestDir::new("parse");
    let user_config = dir.path.join("user.toml");
    fs::write(&user_config, "[daemon]\nauto_start = nope\n").expect("write user config");

    let options = ConfigLoadOptions::from_paths(
        test_paths("/tmp/runtime"),
        dir.path.join("missing-system.toml"),
        user_config.clone(),
    );
    let error = load_config(&options).expect_err("parse error");

    assert!(matches!(error, PersistError::ConfigParse { .. }));
    assert!(error
        .to_string()
        .contains(user_config.to_string_lossy().as_ref()));
}

fn test_paths(runtime_base: &str) -> ConfigPaths {
    ConfigPaths::from_base_dirs(
        PathBuf::from("/home/alice"),
        None,
        None,
        None,
        PathBuf::from(runtime_base),
    )
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "persistshell-config-test-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create test dir");
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
