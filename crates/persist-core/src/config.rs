use std::env;
use std::path::{Path, PathBuf};

use crate::error::{PersistError, Result};

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
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Config {
    pub paths: ConfigPaths,
}

pub fn load_default_config() -> Result<Config> {
    Ok(Config {
        paths: ConfigPaths::from_environment()?,
    })
}

fn runtime_dir_from_uid() -> Result<PathBuf> {
    let uid = env::var("UID").map_err(|_| PersistError::MissingEnvironment {
        name: "XDG_RUNTIME_DIR or UID",
    })?;
    Ok(Path::new("/run/user").join(uid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_xdg_paths_from_home() {
        let paths = ConfigPaths::from_base_dirs(
            PathBuf::from("/home/alice"),
            None,
            None,
            None,
            PathBuf::from("/run/user/1000"),
        );

        assert_eq!(
            paths.config_dir,
            PathBuf::from("/home/alice/.config/persistshell")
        );
        assert_eq!(
            paths.socket_path,
            PathBuf::from("/run/user/1000/persistshell/persist.sock")
        );
    }
}
