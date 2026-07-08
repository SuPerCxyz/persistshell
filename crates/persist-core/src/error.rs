use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, PersistError>;

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
    MissingEnvironment {
        name: &'static str,
    },
    NotImplemented {
        feature: &'static str,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl PersistError {
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

    pub fn not_implemented(feature: &'static str) -> Self {
        Self::NotImplemented { feature }
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
            Self::MissingEnvironment { name } => {
                write!(
                    formatter,
                    "required environment variable is not set: {name}"
                )
            }
            Self::NotImplemented { feature } => {
                write!(formatter, "{feature} is not implemented in this milestone")
            }
            Self::Io { operation, source } => write!(formatter, "{operation} failed: {source}"),
        }
    }
}

impl Error for PersistError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ConfigRead { source, .. } | Self::Io { source, .. } => Some(source),
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
}
