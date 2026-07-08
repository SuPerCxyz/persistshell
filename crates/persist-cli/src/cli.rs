use std::io::{self, Write};
use std::process::ExitCode;

use persist_core::{init_logging, load_default_config, version_string, LoggerConfig, PersistError};

use crate::command::{parse_command, Command};

pub fn run<I>(args: I) -> ExitCode
where
    I: IntoIterator<Item = String>,
{
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let code = run_with_io(args, &mut stdout, &mut stderr);
    ExitCode::from(code)
}

pub fn run_with_io<I, W, E>(args: I, stdout: &mut W, stderr: &mut E) -> u8
where
    I: IntoIterator<Item = String>,
    W: Write,
    E: Write,
{
    if let Err(error) = init_logging(LoggerConfig::default()) {
        let _ = writeln!(stderr, "failed to initialize logging: {error}");
        return 1;
    }

    let args = args.into_iter().skip(1).collect::<Vec<_>>();
    match parse_command(&args).and_then(|command| execute(command, stdout)) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "persist: {error}");
            2
        }
    }
}

fn execute<W: Write>(command: Command, stdout: &mut W) -> Result<(), PersistError> {
    match command {
        Command::Help => write_help(stdout),
        Command::Version => {
            writeln!(stdout, "{}", version_string("persist")).map_err(|source| PersistError::Io {
                operation: "write version",
                source,
            })
        }
        Command::Doctor => write_doctor(stdout),
        Command::Daemon { action } => Err(PersistError::not_implemented(match action.as_deref() {
            Some("start") => "persist daemon start",
            Some("stop") => "persist daemon stop",
            Some("status") => "persist daemon status",
            _ => "persist daemon",
        })),
        Command::Planned { name } => Err(PersistError::not_implemented(planned_feature(&name))),
    }
}

fn planned_feature(name: &str) -> &'static str {
    match name {
        "new" => "persist new",
        "ls" => "persist ls",
        "attach" => "persist attach",
        "detach" => "persist detach",
        "kill" => "persist kill",
        "rename" => "persist rename",
        "install" => "persist install",
        "uninstall" => "persist uninstall",
        _ => "persist command",
    }
}

fn write_help<W: Write>(stdout: &mut W) -> Result<(), PersistError> {
    writeln!(
        stdout,
        "\
PersistShell command line

Usage:
  persist <command>

Available now:
  help       Show this help
  version    Show version information
  doctor     Show local skeleton diagnostics

Planned commands:
  new, ls, attach, detach, kill, rename, daemon, install, uninstall
"
    )
    .map_err(|source| PersistError::Io {
        operation: "write help",
        source,
    })
}

fn write_doctor<W: Write>(stdout: &mut W) -> Result<(), PersistError> {
    let config = load_default_config()?;
    writeln!(stdout, "PersistShell doctor")
        .and_then(|_| writeln!(stdout, "status: engineering skeleton"))
        .and_then(|_| writeln!(stdout, "config_dir: {}", config.paths.config_dir.display()))
        .and_then(|_| writeln!(stdout, "data_dir: {}", config.paths.data_dir.display()))
        .and_then(|_| writeln!(stdout, "state_dir: {}", config.paths.state_dir.display()))
        .and_then(|_| {
            writeln!(
                stdout,
                "runtime_dir: {}",
                config.paths.runtime_dir.display()
            )
        })
        .and_then(|_| writeln!(stdout, "socket: {}", config.paths.socket_path.display()))
        .map_err(|source| PersistError::Io {
            operation: "write doctor output",
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_returns_success() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_with_io(
            ["persist".to_string(), "help".to_string()],
            &mut out,
            &mut err,
        );

        assert_eq!(code, 0);
        assert!(String::from_utf8(out).expect("utf8").contains("Usage"));
        assert!(err.is_empty());
    }

    #[test]
    fn unknown_command_returns_usage_error() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_with_io(
            ["persist".to_string(), "wat".to_string()],
            &mut out,
            &mut err,
        );

        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert!(String::from_utf8(err)
            .expect("utf8")
            .contains("unknown persist command"));
    }
}
