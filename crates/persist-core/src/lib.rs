//! Shared foundations for PersistShell binaries and crates.

pub mod build;
pub mod config;
pub mod error;
pub mod logging;
pub mod ringbuf;
pub mod session;

pub use build::{version_info, version_string, VersionInfo};
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
