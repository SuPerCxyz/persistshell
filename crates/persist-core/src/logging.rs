use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::{self, Visitor};
use serde::Deserialize;

use crate::{PersistError, Result};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    pub const fn as_upper_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }

    fn is_enabled_for(self, configured_level: Self) -> bool {
        self <= configured_level
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" => Ok(Self::Error),
            "warn" | "warning" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            other => Err(format!("unsupported log level: {other}")),
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LogLevel {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(LogLevelVisitor)
    }
}

struct LogLevelVisitor;

impl Visitor<'_> for LogLevelVisitor {
    type Value = LogLevel;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a log level: error, warn, info, debug, or trace")
    }

    fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
    where
        E: de::Error,
    {
        value.parse::<LogLevel>().map_err(E::custom)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LoggerConfig {
    pub level: LogLevel,
    pub destination: LogDestination,
}

impl LoggerConfig {
    pub fn disabled() -> Self {
        Self {
            level: LogLevel::Info,
            destination: LogDestination::Disabled,
        }
    }

    pub fn file(path: PathBuf, level: LogLevel) -> Self {
        Self {
            level,
            destination: LogDestination::File { path },
        }
    }
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LogDestination {
    Disabled,
    File { path: PathBuf },
}

struct FileLogger {
    level: LogLevel,
    path: PathBuf,
    file: File,
}

impl FileLogger {
    fn open(path: PathBuf, level: LogLevel) -> Result<Self> {
        ensure_log_parent(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| PersistError::LogInit {
                path: path.clone(),
                source,
            })?;
        set_file_permission(&path, 0o600)?;

        Ok(Self { level, path, file })
    }

    fn write(&mut self, level: LogLevel, component: &str, message: &str) -> Result<()> {
        if !level.is_enabled_for(self.level) {
            return Ok(());
        }

        let line = format_log_line(level, component, message);
        self.file
            .write_all(line.as_bytes())
            .map_err(|source| PersistError::LogWrite {
                path: self.path.clone(),
                source,
            })
    }

    fn flush(&mut self) -> Result<()> {
        self.file.flush().map_err(|source| PersistError::LogWrite {
            path: self.path.clone(),
            source,
        })
    }
}

static LOGGER: OnceLock<Mutex<Option<FileLogger>>> = OnceLock::new();

/// Initializes internal logging.
///
/// M03 provides a synchronous file-backed logger for daemon/client control-path
/// events. Session output logging remains a later milestone.
pub fn init_logging(config: LoggerConfig) -> Result<()> {
    let logger = match config.destination {
        LogDestination::Disabled => None,
        LogDestination::File { path } => Some(FileLogger::open(path, config.level)?),
    };

    let mut state = logger_state()
        .lock()
        .map_err(|_| PersistError::logger_state())?;
    *state = logger;
    Ok(())
}

pub fn log_message(level: LogLevel, component: &str, message: &str) -> Result<()> {
    let mut state = logger_state()
        .lock()
        .map_err(|_| PersistError::logger_state())?;
    if let Some(logger) = state.as_mut() {
        logger.write(level, component, message)?;
    }
    Ok(())
}

pub fn flush_logging() -> Result<()> {
    let mut state = logger_state()
        .lock()
        .map_err(|_| PersistError::logger_state())?;
    if let Some(logger) = state.as_mut() {
        logger.flush()?;
    }
    Ok(())
}

fn logger_state() -> &'static Mutex<Option<FileLogger>> {
    LOGGER.get_or_init(|| Mutex::new(None))
}

fn ensure_log_parent(path: &Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        PersistError::logger_config(format!(
            "log file path has no parent directory: {}",
            path.display()
        ))
    })?;

    fs::create_dir_all(parent).map_err(|source| PersistError::LogInit {
        path: parent.to_path_buf(),
        source,
    })?;
    set_file_permission(parent, 0o700)
}

#[cfg(unix)]
fn set_file_permission(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|source| {
        PersistError::LogInit {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_file_permission(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn format_log_line(level: LogLevel, component: &str, message: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let component = escape_log_value(component);
    let message = escape_log_value(&sanitize_message(message));

    format!(
        "ts_unix_ms={timestamp} level={} component=\"{component}\" pid={} msg=\"{message}\"\n",
        level.as_upper_str(),
        std::process::id()
    )
}

fn sanitize_message(message: &str) -> String {
    let lowered = message.to_ascii_lowercase();
    const SENSITIVE_MARKERS: &[&str] = &[
        "password",
        "passwd",
        "token",
        "secret",
        "private_key",
        "private key",
    ];

    if SENSITIVE_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        "[REDACTED sensitive log message]".to_string()
    } else {
        message.to_string()
    }
}

fn escape_log_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
fn reset_logging_for_tests() {
    let mut state = logger_state().lock().expect("logger lock");
    *state = None;
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_logger_is_disabled_at_info_level() {
        let config = LoggerConfig::default();

        assert_eq!(config.level, LogLevel::Info);
        assert_eq!(config.destination, LogDestination::Disabled);
    }

    #[test]
    fn parses_log_level_from_config_string() {
        assert_eq!("debug".parse::<LogLevel>().expect("parse"), LogLevel::Debug);
        assert!("verbose".parse::<LogLevel>().is_err());
    }

    #[test]
    fn initializes_file_with_secure_permissions() {
        let _guard = TEST_LOCK.lock().expect("lock");
        let dir = TestDir::new("permissions");
        let path = dir.path.join("nested/client.log");

        init_logging(LoggerConfig::file(path.clone(), LogLevel::Info)).expect("init logging");
        flush_logging().expect("flush");

        assert!(path.exists());
        #[cfg(unix)]
        {
            let dir_mode = fs::metadata(path.parent().expect("parent"))
                .expect("dir metadata")
                .permissions()
                .mode()
                & 0o777;
            let file_mode = fs::metadata(&path)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(dir_mode, 0o700);
            assert_eq!(file_mode, 0o600);
        }
        reset_logging_for_tests();
    }

    #[test]
    fn writes_messages_and_filters_by_level() {
        let _guard = TEST_LOCK.lock().expect("lock");
        let dir = TestDir::new("filter");
        let path = dir.path.join("client.log");

        init_logging(LoggerConfig::file(path.clone(), LogLevel::Info)).expect("init logging");
        log_message(LogLevel::Debug, "client", "debug should be filtered").expect("debug log");
        log_message(LogLevel::Error, "client", "visible error").expect("error log");
        flush_logging().expect("flush");

        let content = fs::read_to_string(&path).expect("read log");
        assert!(content.contains("level=ERROR"));
        assert!(content.contains("component=\"client\""));
        assert!(content.contains("visible error"));
        assert!(!content.contains("debug should be filtered"));
        reset_logging_for_tests();
    }

    #[test]
    fn redacts_sensitive_messages() {
        let _guard = TEST_LOCK.lock().expect("lock");
        let dir = TestDir::new("redact");
        let path = dir.path.join("client.log");

        init_logging(LoggerConfig::file(path.clone(), LogLevel::Trace)).expect("init logging");
        log_message(LogLevel::Info, "client", "token=abc123").expect("info log");
        flush_logging().expect("flush");

        let content = fs::read_to_string(&path).expect("read log");
        assert!(content.contains("[REDACTED sensitive log message]"));
        assert!(!content.contains("abc123"));
        reset_logging_for_tests();
    }

    #[test]
    fn reports_init_error_when_parent_is_file() {
        let _guard = TEST_LOCK.lock().expect("lock");
        let dir = TestDir::new("init-error");
        let parent_file = dir.path.join("not-a-dir");
        fs::write(&parent_file, "x").expect("write parent file");
        let error = init_logging(LoggerConfig::file(
            parent_file.join("client.log"),
            LogLevel::Info,
        ))
        .expect_err("init should fail");

        assert!(matches!(error, PersistError::LogInit { .. }));
        reset_logging_for_tests();
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
            let path = std::env::temp_dir().join(format!(
                "persistshell-logging-test-{name}-{}-{nanos}",
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
}
