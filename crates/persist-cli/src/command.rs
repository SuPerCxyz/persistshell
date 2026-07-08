use persist_core::{PersistError, Result};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Command {
    Help,
    Version,
    Doctor,
    Daemon { action: Option<String> },
    Planned { name: String },
}

pub fn parse_command(args: &[String]) -> Result<Command> {
    match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => Ok(Command::Help),
        Some("-V" | "--version" | "version") => Ok(Command::Version),
        Some("doctor") => Ok(Command::Doctor),
        Some("daemon") => Ok(Command::Daemon {
            action: args.get(1).cloned(),
        }),
        Some(
            name @ ("new" | "ls" | "attach" | "detach" | "kill" | "rename" | "install"
            | "uninstall"),
        ) => Ok(Command::Planned {
            name: name.to_string(),
        }),
        Some(other) => Err(PersistError::invalid_argument(format!(
            "unknown persist command: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_as_help() {
        assert_eq!(parse_command(&[]).expect("parse"), Command::Help);
    }

    #[test]
    fn parses_version_alias() {
        let args = vec!["--version".to_string()];
        assert_eq!(parse_command(&args).expect("parse"), Command::Version);
    }
}
