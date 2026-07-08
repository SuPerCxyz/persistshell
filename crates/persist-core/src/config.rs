mod sections;
#[cfg(test)]
mod tests;
mod values;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{PersistError, Result};

use sections::PartialConfig;
pub use sections::{
    DaemonConfig, LoggingConfig, RingBufferConfig, RuntimeConfig, SecurityConfig, SessionConfig,
    SshConfig,
};
pub use values::{ByteSize, DurationValue};

const CONFIG_FILE_NAME: &str = "config.toml";
const SYSTEM_CONFIG_PATH: &str = "/etc/persistshell/config.toml";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ConfigPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
}

impl ConfigPaths {
    pub fn from_environment() -> Result<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(PersistError::MissingEnvironment { name: "HOME" })?;

        let runtime_base = match env::var_os("XDG_RUNTIME_DIR") {
            Some(value) => PathBuf::from(value),
            None => runtime_dir_from_uid()?,
        };

        Ok(Self::from_base_dirs(
            home,
            env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            env::var_os("XDG_STATE_HOME").map(PathBuf::from),
            runtime_base,
        ))
    }

    pub fn from_base_dirs(
        home: PathBuf,
        config_home: Option<PathBuf>,
        data_home: Option<PathBuf>,
        state_home: Option<PathBuf>,
        runtime_base: PathBuf,
    ) -> Self {
        let config_dir = config_home
            .unwrap_or_else(|| home.join(".config"))
            .join("persistshell");
        let data_dir = data_home
            .unwrap_or_else(|| home.join(".local/share"))
            .join("persistshell");
        let state_dir = state_home
            .unwrap_or_else(|| home.join(".local/state"))
            .join("persistshell");
        let runtime_dir = runtime_base.join("persistshell");
        let socket_path = runtime_dir.join("persist.sock");

        Self {
            config_dir,
            data_dir,
            state_dir,
            runtime_dir,
            socket_path,
        }
    }

    fn set_runtime_dir(&mut self, runtime_dir: PathBuf) {
        self.runtime_dir = runtime_dir;
        self.socket_path = self.runtime_dir.join("persist.sock");
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ConfigLoadOptions {
    pub paths: ConfigPaths,
    pub system_config_path: PathBuf,
    pub user_config_path: PathBuf,
}

impl ConfigLoadOptions {
    pub fn from_environment() -> Result<Self> {
        let paths = ConfigPaths::from_environment()?;
        Ok(Self::from_paths(
            paths.clone(),
            PathBuf::from(SYSTEM_CONFIG_PATH),
            paths.config_dir.join(CONFIG_FILE_NAME),
        ))
    }

    pub fn from_paths(
        paths: ConfigPaths,
        system_config_path: PathBuf,
        user_config_path: PathBuf,
    ) -> Self {
        Self {
            paths,
            system_config_path,
            user_config_path,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Config {
    pub paths: ConfigPaths,
    pub daemon: DaemonConfig,
    pub runtime: RuntimeConfig,
    pub session: SessionConfig,
    pub ring_buffer: RingBufferConfig,
    pub logging: LoggingConfig,
    pub security: SecurityConfig,
    pub ssh: SshConfig,
}

impl Config {
    pub fn default_with_paths(paths: ConfigPaths) -> Self {
        Self {
            runtime: RuntimeConfig {
                socket_dir: paths.runtime_dir.clone(),
            },
            paths,
            daemon: DaemonConfig::default(),
            session: SessionConfig::default(),
            ring_buffer: RingBufferConfig::default(),
            logging: LoggingConfig::default(),
            security: SecurityConfig::default(),
            ssh: SshConfig::default(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        validate_non_empty_path("runtime.socket_dir", &self.runtime.socket_dir)?;
        if !self.runtime.socket_dir.is_absolute() {
            return Err(PersistError::config_validation(
                "runtime.socket_dir must be an absolute path",
            ));
        }

        validate_non_zero_duration("session.kill_grace", self.session.kill_grace)?;
        validate_non_zero_size("ring_buffer.default_size", self.ring_buffer.default_size)?;
        validate_non_zero_size("ring_buffer.max_size", self.ring_buffer.max_size)?;
        validate_non_zero_size("logging.max_file_size", self.logging.max_file_size)?;
        validate_non_zero_duration("logging.flush_interval", self.logging.flush_interval)?;

        if self.daemon.idle_exit {
            validate_non_zero_duration("daemon.idle_exit_after", self.daemon.idle_exit_after)?;
        }
        if self.ring_buffer.default_size > self.ring_buffer.max_size {
            return Err(PersistError::config_validation(
                "ring_buffer.default_size must not exceed ring_buffer.max_size",
            ));
        }
        if self.ring_buffer.replay_bytes > self.ring_buffer.max_size {
            return Err(PersistError::config_validation(
                "ring_buffer.replay_bytes must not exceed ring_buffer.max_size",
            ));
        }
        if self.logging.max_files == 0 {
            return Err(PersistError::config_validation(
                "logging.max_files must be greater than zero",
            ));
        }
        if self.logging.retention_days == 0 {
            return Err(PersistError::config_validation(
                "logging.retention_days must be greater than zero",
            ));
        }
        if self.ssh.bypass_env.trim().is_empty() {
            return Err(PersistError::config_validation(
                "ssh.bypass_env must not be empty",
            ));
        }
        if self.security.allow_root_attach_others {
            return Err(PersistError::config_validation(
                "security.allow_root_attach_others is not supported in Phase 1",
            ));
        }

        Ok(())
    }

    fn apply_partial(&mut self, partial: PartialConfig) -> Result<()> {
        if let Some(daemon) = partial.daemon {
            self.daemon.apply(daemon);
        }
        if let Some(runtime) = partial.runtime {
            self.runtime.apply(runtime)?;
            self.paths.set_runtime_dir(self.runtime.socket_dir.clone());
        }
        if let Some(session) = partial.session {
            self.session.apply(session);
        }
        if let Some(ring_buffer) = partial.ring_buffer {
            self.ring_buffer.apply(ring_buffer);
        }
        if let Some(logging) = partial.logging {
            self.logging.apply(logging);
        }
        if let Some(security) = partial.security {
            self.security.apply(security);
        }
        if let Some(ssh) = partial.ssh {
            self.ssh.apply(ssh);
        }
        Ok(())
    }
}

pub fn load_default_config() -> Result<Config> {
    let config = Config::default_with_paths(ConfigPaths::from_environment()?);
    config.validate()?;
    Ok(config)
}

pub fn load_config(options: &ConfigLoadOptions) -> Result<Config> {
    let mut config = Config::default_with_paths(options.paths.clone());
    apply_config_file(&mut config, &options.system_config_path)?;
    apply_config_file(&mut config, &options.user_config_path)?;
    config.validate()?;
    Ok(config)
}

fn apply_config_file(config: &mut Config, path: &Path) -> Result<()> {
    let Some(partial) = load_partial_config(path)? else {
        return Ok(());
    };
    config.apply_partial(partial)
}

fn load_partial_config(path: &Path) -> Result<Option<PartialConfig>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(PersistError::ConfigRead {
                path: path.to_path_buf(),
                source,
            })
        }
    };

    toml::from_str::<PartialConfig>(&content)
        .map(Some)
        .map_err(|source| PersistError::config_parse(path, source.to_string()))
}

fn expand_uid_template(value: String) -> Result<PathBuf> {
    if value.contains("%UID%") {
        let uid = env::var("UID").map_err(|_| PersistError::MissingEnvironment { name: "UID" })?;
        Ok(PathBuf::from(value.replace("%UID%", &uid)))
    } else {
        Ok(PathBuf::from(value))
    }
}

fn runtime_dir_from_uid() -> Result<PathBuf> {
    let uid = env::var("UID").map_err(|_| PersistError::MissingEnvironment {
        name: "XDG_RUNTIME_DIR or UID",
    })?;
    Ok(Path::new("/run/user").join(uid))
}

fn validate_non_empty_path(name: &'static str, path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        Err(PersistError::config_validation(format!(
            "{name} must not be empty"
        )))
    } else {
        Ok(())
    }
}

fn validate_non_zero_size(name: &'static str, value: ByteSize) -> Result<()> {
    if value.bytes() == 0 {
        Err(PersistError::config_validation(format!(
            "{name} must be greater than zero"
        )))
    } else {
        Ok(())
    }
}

fn validate_non_zero_duration(name: &'static str, value: DurationValue) -> Result<()> {
    if value.duration().is_zero() {
        Err(PersistError::config_validation(format!(
            "{name} must be greater than zero"
        )))
    } else {
        Ok(())
    }
}
