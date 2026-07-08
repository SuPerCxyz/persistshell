use crate::Result;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct LoggerConfig {
    pub level: LogLevel,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
        }
    }
}

/// Initializes internal logging.
///
/// M01 intentionally keeps this as a no-op framework hook. Real daemon/client
/// logging is introduced by the logging milestone.
pub fn init_logging(_config: LoggerConfig) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_level_is_info() {
        assert_eq!(LoggerConfig::default().level, LogLevel::Info);
    }
}
