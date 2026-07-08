//! Shared foundations for PersistShell binaries and crates.

pub mod build;
pub mod config;
pub mod error;
pub mod logging;
pub mod session;

pub use build::{version_info, version_string, VersionInfo};
pub use config::{
    load_config, load_default_config, ByteSize, Config, ConfigLoadOptions, ConfigPaths,
    DurationValue,
};
pub use error::{PersistError, Result};
pub use logging::{init_logging, LogLevel, LoggerConfig};
pub use session::{AttachMode, SessionStatus};
