//! Shared foundations for PersistShell binaries and crates.

pub mod build;
pub mod command_history;
mod command_history_format;
pub mod config;
pub mod error;
pub mod logging;
pub mod ringbuf;
pub mod session;

pub use build::{version_info, version_string, VersionInfo};
pub use command_history::{
    append_command, command_count, command_history_path, read_commands_desc, CommandRecord,
    MAX_COMMAND_BYTES, MAX_HISTORY_BYTES, MAX_HISTORY_RECORDS,
};
pub use config::{
    load_config, load_default_config, ByteSize, Config, ConfigLoadOptions, ConfigPaths,
    DurationValue,
};
pub use error::{PersistError, Result};
pub use logging::{
    flush_logging, init_logging, log_message, LogDestination, LogLevel, LoggerConfig,
};
pub use ringbuf::RingBuffer;
pub use session::{AttachMode, SessionStatus};

pub mod pidfile;

#[cfg(test)]
mod command_history_tests;
