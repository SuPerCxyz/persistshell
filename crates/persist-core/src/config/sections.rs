use std::path::PathBuf;

use serde::Deserialize;

use super::{expand_uid_template, ByteSize, DurationValue};
use crate::error::Result;
use crate::logging::{LogLevel, LoggerConfig};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DaemonConfig {
    pub auto_start: bool,
    pub idle_exit: bool,
    pub idle_exit_after: DurationValue,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            auto_start: true,
            idle_exit: true,
            idle_exit_after: DurationValue::from_secs(10 * 60),
        }
    }
}

impl DaemonConfig {
    pub(super) fn apply(&mut self, partial: PartialDaemonConfig) {
        if let Some(value) = partial.auto_start {
            self.auto_start = value;
        }
        if let Some(value) = partial.idle_exit {
            self.idle_exit = value;
        }
        if let Some(value) = partial.idle_exit_after {
            self.idle_exit_after = value;
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeConfig {
    pub socket_dir: PathBuf,
}

impl RuntimeConfig {
    pub(super) fn apply(&mut self, partial: PartialRuntimeConfig) -> Result<()> {
        if let Some(value) = partial.socket_dir {
            self.socket_dir = expand_uid_template(value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SessionConfig {
    pub new_session_on_ssh: bool,
    pub default_shell: String,
    pub kill_grace: DurationValue,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            new_session_on_ssh: true,
            default_shell: String::new(),
            kill_grace: DurationValue::from_secs(3),
        }
    }
}

impl SessionConfig {
    pub(super) fn apply(&mut self, partial: PartialSessionConfig) {
        if let Some(value) = partial.new_session_on_ssh {
            self.new_session_on_ssh = value;
        }
        if let Some(value) = partial.default_shell {
            self.default_shell = value;
        }
        if let Some(value) = partial.kill_grace {
            self.kill_grace = value;
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RingBufferConfig {
    pub default_size: ByteSize,
    pub max_size: ByteSize,
    pub replay_on_attach: bool,
    pub replay_bytes: ByteSize,
}

impl Default for RingBufferConfig {
    fn default() -> Self {
        Self {
            default_size: ByteSize::from_bytes(8 * 1024 * 1024),
            max_size: ByteSize::from_bytes(128 * 1024 * 1024),
            replay_on_attach: true,
            replay_bytes: ByteSize::from_bytes(512 * 1024),
        }
    }
}

impl RingBufferConfig {
    pub(super) fn apply(&mut self, partial: PartialRingBufferConfig) {
        if let Some(value) = partial.default_size {
            self.default_size = value;
        }
        if let Some(value) = partial.max_size {
            self.max_size = value;
        }
        if let Some(value) = partial.replay_on_attach {
            self.replay_on_attach = value;
        }
        if let Some(value) = partial.replay_bytes {
            self.replay_bytes = value;
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LoggingConfig {
    pub session_log: bool,
    pub max_file_size: ByteSize,
    pub max_files: u32,
    pub retention_days: u32,
    pub flush_interval: DurationValue,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            session_log: true,
            max_file_size: ByteSize::from_bytes(100 * 1024 * 1024),
            max_files: 10,
            retention_days: 30,
            flush_interval: DurationValue::from_secs(1),
        }
    }
}

impl LoggingConfig {
    pub(super) fn apply(&mut self, partial: PartialLoggingConfig) {
        if let Some(value) = partial.session_log {
            self.session_log = value;
        }
        if let Some(value) = partial.max_file_size {
            self.max_file_size = value;
        }
        if let Some(value) = partial.max_files {
            self.max_files = value;
        }
        if let Some(value) = partial.retention_days {
            self.retention_days = value;
        }
        if let Some(value) = partial.flush_interval {
            self.flush_interval = value;
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InternalLogConfig {
    pub level: LogLevel,
    pub daemon_log: PathBuf,
    pub client_log: PathBuf,
    pub max_file_size: ByteSize,
    pub max_files: u32,
}

impl InternalLogConfig {
    pub fn default_with_state_dir(state_dir: &std::path::Path) -> Self {
        Self {
            level: LogLevel::Info,
            daemon_log: state_dir.join("daemon.log"),
            client_log: state_dir.join("client.log"),
            max_file_size: ByteSize::from_bytes(20 * 1024 * 1024),
            max_files: 5,
        }
    }

    pub fn daemon_logger_config(&self) -> LoggerConfig {
        LoggerConfig::file(self.daemon_log.clone(), self.level)
    }

    pub fn client_logger_config(&self) -> LoggerConfig {
        LoggerConfig::file(self.client_log.clone(), self.level)
    }

    pub(super) fn apply(&mut self, partial: PartialInternalLogConfig) -> Result<()> {
        if let Some(value) = partial.level {
            self.level = value;
        }
        if let Some(value) = partial.daemon_log {
            self.daemon_log = expand_uid_template(value)?;
        }
        if let Some(value) = partial.client_log {
            self.client_log = expand_uid_template(value)?;
        }
        if let Some(value) = partial.max_file_size {
            self.max_file_size = value;
        }
        if let Some(value) = partial.max_files {
            self.max_files = value;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct SecurityConfig {
    pub allow_root_attach_others: bool,
    pub enable_input_recording: bool,
}

impl SecurityConfig {
    pub(super) fn apply(&mut self, partial: PartialSecurityConfig) {
        if let Some(value) = partial.allow_root_attach_others {
            self.allow_root_attach_others = value;
        }
        if let Some(value) = partial.enable_input_recording {
            self.enable_input_recording = value;
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SshConfig {
    pub auto_hook: bool,
    pub bypass_env: String,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            auto_hook: true,
            bypass_env: "PERSIST_DISABLE".to_string(),
        }
    }
}

impl SshConfig {
    pub(super) fn apply(&mut self, partial: PartialSshConfig) {
        if let Some(value) = partial.auto_hook {
            self.auto_hook = value;
        }
        if let Some(value) = partial.bypass_env {
            self.bypass_env = value;
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialConfig {
    pub(super) daemon: Option<PartialDaemonConfig>,
    pub(super) runtime: Option<PartialRuntimeConfig>,
    pub(super) session: Option<PartialSessionConfig>,
    pub(super) ring_buffer: Option<PartialRingBufferConfig>,
    pub(super) logging: Option<PartialLoggingConfig>,
    pub(super) internal_log: Option<PartialInternalLogConfig>,
    pub(super) security: Option<PartialSecurityConfig>,
    pub(super) ssh: Option<PartialSshConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialDaemonConfig {
    auto_start: Option<bool>,
    idle_exit: Option<bool>,
    idle_exit_after: Option<DurationValue>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialRuntimeConfig {
    socket_dir: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialSessionConfig {
    new_session_on_ssh: Option<bool>,
    default_shell: Option<String>,
    kill_grace: Option<DurationValue>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialRingBufferConfig {
    default_size: Option<ByteSize>,
    max_size: Option<ByteSize>,
    replay_on_attach: Option<bool>,
    replay_bytes: Option<ByteSize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialLoggingConfig {
    session_log: Option<bool>,
    max_file_size: Option<ByteSize>,
    max_files: Option<u32>,
    retention_days: Option<u32>,
    flush_interval: Option<DurationValue>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialInternalLogConfig {
    level: Option<LogLevel>,
    daemon_log: Option<String>,
    client_log: Option<String>,
    max_file_size: Option<ByteSize>,
    max_files: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialSecurityConfig {
    allow_root_attach_others: Option<bool>,
    enable_input_recording: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialSshConfig {
    auto_hook: Option<bool>,
    bypass_env: Option<String>,
}
