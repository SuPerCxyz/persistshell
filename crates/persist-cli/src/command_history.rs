use std::io::{Read, Take};

use persist_core::{append_command, command_history_path, Config, PersistError, MAX_COMMAND_BYTES};

pub fn append_from_reader<R: Read>(
    config: &Config,
    session_id: u32,
    shell: &str,
    reader: R,
) -> Result<(), PersistError> {
    let command = read_bounded(reader)?;
    let path = command_history_path(&config.paths.data_dir, session_id);
    append_command(&path, shell, &command)?;
    Ok(())
}

fn read_bounded<R: Read>(reader: R) -> Result<Vec<u8>, PersistError> {
    let mut command = Vec::new();
    let mut limited: Take<R> = reader.take((MAX_COMMAND_BYTES + 1) as u64);
    limited
        .read_to_end(&mut command)
        .map_err(|source| PersistError::Io {
            operation: "read history command",
            source,
        })?;
    if command.len() > MAX_COMMAND_BYTES {
        return Err(PersistError::invalid_argument(
            "history command is too large",
        ));
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use persist_core::{command_history_path, read_commands_desc, ConfigPaths};

    use super::*;

    #[test]
    fn appends_stdin_without_trimming_multiline_command() {
        let paths = test_paths("append");
        let config = Config::default_with_paths(paths.clone());
        append_from_reader(&config, 9, "bash", Cursor::new(b"echo one\necho two")).unwrap();

        let records = read_commands_desc(&command_history_path(&paths.data_dir, 9), 0, 1).unwrap();
        assert_eq!(records[0].command, b"echo one\necho two");
        let _ = std::fs::remove_dir_all(paths.data_dir.parent().unwrap());
    }

    #[test]
    fn rejects_oversized_stdin() {
        let paths = test_paths("oversized");
        let config = Config::default_with_paths(paths.clone());
        let input = vec![b'x'; MAX_COMMAND_BYTES + 1];
        assert!(append_from_reader(&config, 10, "zsh", Cursor::new(input)).is_err());
        let _ = std::fs::remove_dir_all(paths.data_dir.parent().unwrap());
    }

    fn test_paths(name: &str) -> ConfigPaths {
        let root = std::env::temp_dir().join(format!(
            "persistshell-cli-history-{name}-{}",
            std::process::id()
        ));
        ConfigPaths::from_base_dirs(
            root.join("home"),
            Some(root.join("config")),
            Some(root.join("data")),
            Some(root.join("state")),
            root.join("run"),
        )
    }
}
