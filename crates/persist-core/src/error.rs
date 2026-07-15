use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, PersistError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidArgument,
    UnknownCommand,
    ConfigParse,
    ConfigRead,
    ConfigInvalid,
    MissingEnvironment,
    UnsupportedEnvironment,
    DaemonNotRunning,
    DaemonAlreadyRunning,
    SocketPermission,
    SocketMissing,
    SessionNotFound,
    SessionBusy,
    PtyOpenFailed,
    ForkFailed,
    ExecFailed,
    IoctlFailed,
    MetadataOpenFailed,
    MetadataCorrupt,
    ProtocolVersion,
    InvalidFrame,
    RequestTimeout,
    LogConfig,
    LogInit,
    LoggerState,
    LogWrite,
    Internal,
    NotImplemented,
    Io,
}

impl ErrorCode {
    pub fn code_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "E_INVALID_ARGUMENT",
            Self::UnknownCommand => "E_UNKNOWN_COMMAND",
            Self::ConfigParse => "E_CONFIG_PARSE",
            Self::ConfigRead => "E_CONFIG_READ",
            Self::ConfigInvalid => "E_CONFIG_INVALID",
            Self::MissingEnvironment => "E_MISSING_ENVIRONMENT",
            Self::UnsupportedEnvironment => "E_UNSUPPORTED_ENVIRONMENT",
            Self::DaemonNotRunning => "E_DAEMON_NOT_RUNNING",
            Self::DaemonAlreadyRunning => "E_DAEMON_ALREADY_RUNNING",
            Self::SocketPermission => "E_SOCKET_PERMISSION",
            Self::SocketMissing => "E_SOCKET_MISSING",
            Self::SessionNotFound => "E_SESSION_NOT_FOUND",
            Self::SessionBusy => "E_SESSION_BUSY",
            Self::PtyOpenFailed => "E_PTY_OPEN_FAILED",
            Self::ForkFailed => "E_FORK_FAILED",
            Self::ExecFailed => "E_EXEC_FAILED",
            Self::IoctlFailed => "E_IOCTL_FAILED",
            Self::MetadataOpenFailed => "E_METADATA_OPEN_FAILED",
            Self::MetadataCorrupt => "E_METADATA_CORRUPT",
            Self::ProtocolVersion => "E_PROTOCOL_VERSION",
            Self::InvalidFrame => "E_INVALID_FRAME",
            Self::RequestTimeout => "E_REQUEST_TIMEOUT",
            Self::LogConfig => "E_LOG_CONFIG",
            Self::LogInit => "E_LOG_INIT",
            Self::LoggerState => "E_LOGGER_STATE",
            Self::LogWrite => "E_LOG_WRITE",
            Self::Internal => "E_INTERNAL",
            Self::NotImplemented => "E_NOT_IMPLEMENTED",
            Self::Io => "E_IO",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::InvalidArgument => "invalid command-line argument or input",
            Self::UnknownCommand => "unrecognized persist command",
            Self::ConfigParse => "failed to parse configuration file",
            Self::ConfigRead => "failed to read configuration file",
            Self::ConfigInvalid => "configuration validation failed",
            Self::MissingEnvironment => "required environment variable is not set",
            Self::UnsupportedEnvironment => "unsupported operating environment",
            Self::DaemonNotRunning => "PersistShell daemon is not running",
            Self::DaemonAlreadyRunning => "PersistShell daemon is already running",
            Self::SocketPermission => "Unix socket permission denied",
            Self::SocketMissing => "Unix socket does not exist",
            Self::SessionNotFound => "specified session was not found",
            Self::SessionBusy => "session is currently busy",
            Self::PtyOpenFailed => "failed to open pseudo-terminal",
            Self::ForkFailed => "process fork failed",
            Self::ExecFailed => "failed to execute shell process",
            Self::IoctlFailed => "terminal ioctl operation failed",
            Self::MetadataOpenFailed => "failed to open metadata store",
            Self::MetadataCorrupt => "metadata store is corrupted",
            Self::ProtocolVersion => "protocol version mismatch between client and daemon",
            Self::InvalidFrame => "received an invalid protocol frame",
            Self::RequestTimeout => "request timed out",
            Self::LogConfig => "logging configuration error",
            Self::LogInit => "failed to initialize log file",
            Self::LoggerState => "internal logger state is unavailable",
            Self::LogWrite => "failed to write to log file",
            Self::Internal => "internal error, please report this as a bug",
            Self::NotImplemented => "feature is not yet implemented",
            Self::Io => "input/output operation failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    UserError,
    EnvironmentError,
    SyscallError,
    ProtocolError,
    InternalError,
}

impl ErrorKind {
    pub fn exit_code(self) -> u8 {
        match self {
            Self::UserError => 1,
            Self::EnvironmentError => 2,
            Self::SyscallError => 3,
            Self::ProtocolError => 4,
            Self::InternalError => 5,
        }
    }
}

#[derive(Debug)]
pub enum PersistError {
    ConfigParse {
        path: PathBuf,
        message: String,
    },
    ConfigRead {
        path: PathBuf,
        source: io::Error,
    },
    ConfigValidation {
        message: String,
    },
    InvalidArgument {
        message: String,
    },
    LogConfig {
        message: String,
    },
    LogInit {
        path: PathBuf,
        source: io::Error,
    },
    LoggerState {
        message: String,
    },
    LogWrite {
        path: PathBuf,
        source: io::Error,
    },
    MissingEnvironment {
        name: &'static str,
    },
    NotImplemented {
        feature: &'static str,
    },
    DaemonNotRunning,
    DaemonAlreadyRunning,
    MetadataOpen {
        path: PathBuf,
        message: String,
    },
    MetadataOperation {
        operation: &'static str,
        message: String,
    },
    Internal {
        message: String,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl PersistError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::ConfigParse { .. } => ErrorCode::ConfigParse,
            Self::ConfigRead { .. } => ErrorCode::ConfigRead,
            Self::ConfigValidation { .. } => ErrorCode::ConfigInvalid,
            Self::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Self::LogConfig { .. } => ErrorCode::LogConfig,
            Self::LogInit { .. } => ErrorCode::LogInit,
            Self::LoggerState { .. } => ErrorCode::LoggerState,
            Self::LogWrite { .. } => ErrorCode::LogWrite,
            Self::MissingEnvironment { .. } => ErrorCode::MissingEnvironment,
            Self::NotImplemented { .. } => ErrorCode::NotImplemented,
            Self::DaemonNotRunning => ErrorCode::DaemonNotRunning,
            Self::DaemonAlreadyRunning => ErrorCode::DaemonAlreadyRunning,
            Self::MetadataOpen { .. } => ErrorCode::MetadataOpenFailed,
            Self::MetadataOperation { .. } => ErrorCode::Io,
            Self::Internal { .. } => ErrorCode::Internal,
            Self::Io { .. } => ErrorCode::Io,
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidArgument { .. } | Self::ConfigValidation { .. } => ErrorKind::UserError,
            Self::MissingEnvironment { .. }
            | Self::ConfigParse { .. }
            | Self::ConfigRead { .. } => ErrorKind::EnvironmentError,
            Self::Io { .. } | Self::MetadataOperation { .. } => ErrorKind::SyscallError,
            Self::MetadataOpen { .. } => ErrorKind::EnvironmentError,
            Self::LogInit { .. } | Self::LogWrite { .. } => ErrorKind::EnvironmentError,
            Self::LogConfig { .. } | Self::LoggerState { .. } => ErrorKind::InternalError,
            Self::DaemonNotRunning => ErrorKind::EnvironmentError,
            Self::DaemonAlreadyRunning => ErrorKind::EnvironmentError,
            Self::NotImplemented { .. } => ErrorKind::InternalError,
            Self::Internal { .. } => ErrorKind::InternalError,
        }
    }

    pub fn exit_code(&self) -> u8 {
        self.kind().exit_code()
    }

    pub fn user_facing(&self, app: &str) -> String {
        let code = self.code();
        let kind = self.kind();
        let mut output = String::new();

        output.push_str(&format!("{}: {} [{}]", app, code.code_str(), self));

        if let Some(suggestion) = self.suggestion() {
            output.push_str(&format!("\n建议: {}", suggestion));
        }

        if kind == ErrorKind::InternalError {
            output.push_str("\n请报告此问题: https://github.com/SuPerCxyz/persistshell/issues");
        }

        output
    }

    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::ConfigParse { .. } | Self::ConfigValidation { .. } => {
                Some("请检查配置文件格式，或执行 persist doctor 诊断")
            }
            Self::ConfigRead { .. } => Some("请检查配置文件的读取权限，或执行 persist doctor 诊断"),
            Self::InvalidArgument { .. } => Some("执行 persist help 查看命令用法"),
            Self::MissingEnvironment { .. } => {
                Some("请确保所需环境变量已正确设置，或执行 persist doctor 诊断")
            }
            Self::Io { .. } | Self::LogInit { .. } | Self::LogWrite { .. } => {
                Some("请检查文件系统和权限，或执行 persist doctor 诊断")
            }
            Self::DaemonNotRunning => Some("执行 persist daemon start 启动 daemon"),
            Self::DaemonAlreadyRunning => Some("daemon 已在运行中"),
            Self::LogConfig { .. }
            | Self::LoggerState { .. }
            | Self::Internal { .. }
            | Self::MetadataOpen { .. }
            | Self::MetadataOperation { .. } => None,
            Self::NotImplemented { .. } => Some("该功能将在后续版本中实现"),
        }
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument {
            message: message.into(),
        }
    }

    pub fn config_parse(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::ConfigParse {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn config_validation(message: impl Into<String>) -> Self {
        Self::ConfigValidation {
            message: message.into(),
        }
    }

    pub fn logger_config(message: impl Into<String>) -> Self {
        Self::LogConfig {
            message: message.into(),
        }
    }

    pub fn logger_state() -> Self {
        Self::LoggerState {
            message: "internal logger state is unavailable".to_string(),
        }
    }

    pub fn not_implemented(feature: &'static str) -> Self {
        Self::NotImplemented { feature }
    }

    pub fn daemon_not_running() -> Self {
        Self::DaemonNotRunning
    }

    pub fn daemon_already_running() -> Self {
        Self::DaemonAlreadyRunning
    }
}

impl fmt::Display for PersistError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfigParse { path, message } => {
                write!(
                    formatter,
                    "config parse error in {}: {message}",
                    path.display()
                )
            }
            Self::ConfigRead { path, source } => {
                write!(
                    formatter,
                    "failed to read config file {}: {source}",
                    path.display()
                )
            }
            Self::ConfigValidation { message } => {
                write!(formatter, "config validation error: {message}")
            }
            Self::InvalidArgument { message } => write!(formatter, "invalid argument: {message}"),
            Self::LogConfig { message } => {
                write!(formatter, "logging configuration error: {message}")
            }
            Self::LogInit { path, source } => {
                write!(
                    formatter,
                    "failed to initialize log file {}: {source}",
                    path.display()
                )
            }
            Self::LoggerState { message } => write!(formatter, "logger state error: {message}"),
            Self::LogWrite { path, source } => {
                write!(
                    formatter,
                    "failed to write log file {}: {source}",
                    path.display()
                )
            }
            Self::MissingEnvironment { name } => {
                write!(
                    formatter,
                    "required environment variable is not set: {name}"
                )
            }
            Self::NotImplemented { feature } => {
                write!(formatter, "{feature} is not implemented in this milestone")
            }
            Self::DaemonNotRunning => write!(formatter, "daemon is not running"),
            Self::DaemonAlreadyRunning => write!(formatter, "daemon is already running"),
            Self::Internal { message } => write!(formatter, "internal error: {message}"),
            Self::Io { operation, source } => write!(formatter, "{operation} failed: {source}"),
            Self::MetadataOpen { path, message } => {
                write!(
                    formatter,
                    "failed to open metadata store at {}: {message}",
                    path.display()
                )
            }
            Self::MetadataOperation { operation, message } => {
                write!(formatter, "{operation} failed: {message}")
            }
        }
    }
}

impl Error for PersistError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ConfigRead { source, .. }
            | Self::Io { source, .. }
            | Self::LogInit { source, .. }
            | Self::LogWrite { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_implemented_has_feature_name() {
        let error = PersistError::not_implemented("pty engine");
        assert!(error.to_string().contains("pty engine"));
    }

    #[test]
    fn invalid_argument_code_and_kind() {
        let error = PersistError::invalid_argument("bad arg");
        assert_eq!(error.code(), ErrorCode::InvalidArgument);
        assert_eq!(error.kind(), ErrorKind::UserError);
        assert_eq!(error.exit_code(), 1);
    }

    #[test]
    fn config_validation_code_and_kind() {
        let error = PersistError::config_validation("invalid value");
        assert_eq!(error.code(), ErrorCode::ConfigInvalid);
        assert_eq!(error.kind(), ErrorKind::UserError);
        assert_eq!(error.exit_code(), 1);
    }

    #[test]
    fn missing_environment_code_and_kind() {
        let error = PersistError::MissingEnvironment { name: "HOME" };
        assert_eq!(error.code(), ErrorCode::MissingEnvironment);
        assert_eq!(error.kind(), ErrorKind::EnvironmentError);
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn not_implemented_code_and_kind() {
        let error = PersistError::not_implemented("feature x");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
        assert_eq!(error.kind(), ErrorKind::InternalError);
        assert_eq!(error.exit_code(), 5);
    }

    #[test]
    fn io_error_code_and_kind() {
        let io_error = io::Error::new(io::ErrorKind::PermissionDenied, "test");
        let error = PersistError::Io {
            operation: "write",
            source: io_error,
        };
        assert_eq!(error.code(), ErrorCode::Io);
        assert_eq!(error.kind(), ErrorKind::SyscallError);
        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn log_init_is_environment_error() {
        let io_error = io::Error::new(io::ErrorKind::NotFound, "no such file");
        let error = PersistError::LogInit {
            path: PathBuf::from("/var/log/persist.log"),
            source: io_error,
        };
        assert_eq!(error.kind(), ErrorKind::EnvironmentError);
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn logger_state_is_internal_error() {
        let error = PersistError::logger_state();
        assert_eq!(error.kind(), ErrorKind::InternalError);
        assert_eq!(error.exit_code(), 5);
    }

    #[test]
    fn user_facing_formatted_output() {
        let error = PersistError::invalid_argument("bad arg");
        let output = error.user_facing("persist");
        assert!(output.contains("persist: E_INVALID_ARGUMENT"));
        assert!(output.contains("bad arg"));
        assert!(output.contains("建议:"));
        assert!(output.contains("persist help"));
    }

    #[test]
    fn user_facing_internal_has_bug_report() {
        let error = PersistError::not_implemented("pty engine");
        let output = error.user_facing("persistd");
        assert!(output.contains("persistd: E_NOT_IMPLEMENTED"));
        assert!(output.contains("github.com"));
    }

    #[test]
    fn error_code_str_is_consistent() {
        for code in &[
            ErrorCode::InvalidArgument,
            ErrorCode::ConfigParse,
            ErrorCode::ConfigRead,
            ErrorCode::ConfigInvalid,
            ErrorCode::MissingEnvironment,
            ErrorCode::DaemonNotRunning,
            ErrorCode::SessionNotFound,
            ErrorCode::SessionBusy,
            ErrorCode::PtyOpenFailed,
            ErrorCode::DaemonNotRunning,
            ErrorCode::DaemonAlreadyRunning,
            ErrorCode::ForkFailed,
            ErrorCode::ExecFailed,
            ErrorCode::MetadataOpenFailed,
            ErrorCode::ProtocolVersion,
            ErrorCode::InvalidFrame,
            ErrorCode::Internal,
            ErrorCode::NotImplemented,
            ErrorCode::Io,
        ] {
            let s = code.code_str();
            assert!(s.starts_with("E_"), "code {:?} has invalid str {s}", code);
        }
    }

    #[test]
    fn every_error_variant_has_mapped_code() {
        fn check(error: PersistError) {
            let code = error.code();
            assert!(!code.code_str().is_empty());
            assert!(!code.description().is_empty());
        }

        check(PersistError::ConfigParse {
            path: PathBuf::from("a"),
            message: "b".into(),
        });
        check(PersistError::ConfigRead {
            path: PathBuf::from("a"),
            source: io::Error::other("err"),
        });
        check(PersistError::ConfigValidation {
            message: "b".into(),
        });
        check(PersistError::InvalidArgument {
            message: "b".into(),
        });
        check(PersistError::LogConfig {
            message: "b".into(),
        });
        check(PersistError::LogInit {
            path: PathBuf::from("a"),
            source: io::Error::other("err"),
        });
        check(PersistError::LoggerState {
            message: "b".into(),
        });
        check(PersistError::LogWrite {
            path: PathBuf::from("a"),
            source: io::Error::other("err"),
        });
        check(PersistError::MissingEnvironment { name: "HOME" });
        check(PersistError::NotImplemented { feature: "x" });
        check(PersistError::DaemonNotRunning);
        check(PersistError::DaemonAlreadyRunning);
        check(PersistError::Internal {
            message: "internal".into(),
        });
        check(PersistError::Io {
            operation: "op",
            source: io::Error::other("err"),
        });
    }

    #[test]
    fn exit_code_consistency() {
        let test_cases: Vec<(PersistError, u8)> = vec![
            (
                PersistError::InvalidArgument {
                    message: "x".into(),
                },
                1,
            ),
            (
                PersistError::ConfigValidation {
                    message: "x".into(),
                },
                1,
            ),
            (PersistError::MissingEnvironment { name: "HOME" }, 2),
            (
                PersistError::ConfigParse {
                    path: PathBuf::from("a"),
                    message: "b".into(),
                },
                2,
            ),
            (
                PersistError::ConfigRead {
                    path: PathBuf::from("a"),
                    source: io::Error::other("err"),
                },
                2,
            ),
            (
                PersistError::Io {
                    operation: "op",
                    source: io::Error::other("err"),
                },
                3,
            ),
            (PersistError::NotImplemented { feature: "x" }, 5),
            (PersistError::DaemonNotRunning, 2),
            (PersistError::DaemonAlreadyRunning, 2),
        ];

        for (error, expected) in test_cases {
            assert_eq!(
                error.exit_code(),
                expected,
                "exit_code mismatch for {:?}",
                error.code()
            );
        }
    }

    #[test]
    fn log_init_and_write_have_suggestions() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let init = PersistError::LogInit {
            path: PathBuf::from("/var/log/persist.log"),
            source: io_err,
        };
        assert!(init.suggestion().is_some());

        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let write = PersistError::LogWrite {
            path: PathBuf::from("/var/log/persist.log"),
            source: io_err,
        };
        assert!(write.suggestion().is_some());
    }
}
