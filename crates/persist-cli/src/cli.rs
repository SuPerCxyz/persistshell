use std::io::{self, Write};
use std::process::ExitCode;

use persist_core::{
    init_logging, load_config, log_message, version_string, Config, ConfigLoadOptions, LogLevel,
    LoggerConfig, PersistError,
};

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
    let args = args.into_iter().skip(1).collect::<Vec<_>>();
    match parse_command(&args).and_then(|command| execute_with_optional_config(command, stdout)) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(stderr, "persist: {error}");
            2
        }
    }
}

fn execute_with_optional_config<W: Write>(
    command: Command,
    stdout: &mut W,
) -> Result<(), PersistError> {
    if command_uses_config(&command) {
        let options = ConfigLoadOptions::from_environment()?;
        let config = load_config(&options)?;
        init_logging(config.internal_log.client_logger_config())?;
        log_message(LogLevel::Info, "client", command_log_message(&command))?;
        execute(command, stdout, Some((&options, &config)))
    } else {
        init_logging(LoggerConfig::disabled())?;
        execute(command, stdout, None)
    }
}

fn command_uses_config(command: &Command) -> bool {
    matches!(
        command,
        Command::Doctor | Command::Config | Command::Daemon { .. } | Command::Planned { .. }
    )
}

fn command_log_message(command: &Command) -> &'static str {
    match command {
        Command::Help => "command=help started",
        Command::Version => "command=version started",
        Command::Doctor => "command=doctor started",
        Command::Config => "command=config started",
        Command::Daemon { .. } => "command=daemon started",
        Command::Planned { .. } => "command=planned started",
    }
}

fn execute<W: Write>(
    command: Command,
    stdout: &mut W,
    loaded_config: Option<(&ConfigLoadOptions, &Config)>,
) -> Result<(), PersistError> {
    match command {
        Command::Help => write_help(stdout),
        Command::Version => {
            writeln!(stdout, "{}", version_string("persist")).map_err(|source| PersistError::Io {
                operation: "write version",
                source,
            })
        }
        Command::Doctor => write_doctor(stdout, loaded_config.map(|(_, config)| config)),
        Command::Config => write_config(stdout, loaded_config),
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
  config     Show effective configuration

Planned commands:
  new, ls, attach, detach, kill, rename, daemon, install, uninstall
"
    )
    .map_err(|source| PersistError::Io {
        operation: "write help",
        source,
    })
}

fn write_config<W: Write>(
    stdout: &mut W,
    loaded_config: Option<(&ConfigLoadOptions, &Config)>,
) -> Result<(), PersistError> {
    if let Some((options, config)) = loaded_config {
        write_config_values(stdout, options, config)
    } else {
        let options = ConfigLoadOptions::from_environment()?;
        let config = load_config(&options)?;
        write_config_values(stdout, &options, &config)
    }
}

fn write_config_values<W: Write>(
    stdout: &mut W,
    options: &ConfigLoadOptions,
    config: &Config,
) -> Result<(), PersistError> {
    write_config_values_inner(stdout, options, config).map_err(|source| PersistError::Io {
        operation: "write config output",
        source,
    })
}

fn write_config_values_inner<W: Write>(
    stdout: &mut W,
    options: &ConfigLoadOptions,
    config: &Config,
) -> io::Result<()> {
    writeln!(stdout, "PersistShell config")?;
    write_config_sources(stdout, options)?;
    write_config_paths(stdout, config)?;
    write_daemon_config(stdout, config)?;
    write_runtime_config(stdout, config)?;
    write_session_config(stdout, config)?;
    write_ring_buffer_config(stdout, config)?;
    write_logging_config(stdout, config)?;
    write_internal_log_config(stdout, config)?;
    write_security_config(stdout, config)?;
    write_ssh_config(stdout, config)
}

fn write_config_sources<W: Write>(stdout: &mut W, options: &ConfigLoadOptions) -> io::Result<()> {
    writeln!(
        stdout,
        "system_config: {}",
        options.system_config_path.display()
    )?;
    writeln!(
        stdout,
        "user_config: {}",
        options.user_config_path.display()
    )
}

fn write_config_paths<W: Write>(stdout: &mut W, config: &Config) -> io::Result<()> {
    writeln!(stdout, "config_dir: {}", config.paths.config_dir.display())?;
    writeln!(stdout, "data_dir: {}", config.paths.data_dir.display())?;
    writeln!(stdout, "state_dir: {}", config.paths.state_dir.display())?;
    writeln!(
        stdout,
        "runtime_dir: {}",
        config.paths.runtime_dir.display()
    )?;
    writeln!(stdout, "socket: {}", config.paths.socket_path.display())
}

fn write_daemon_config<W: Write>(stdout: &mut W, config: &Config) -> io::Result<()> {
    writeln!(stdout, "daemon.auto_start: {}", config.daemon.auto_start)?;
    writeln!(stdout, "daemon.idle_exit: {}", config.daemon.idle_exit)?;
    writeln!(
        stdout,
        "daemon.idle_exit_after: {}",
        config.daemon.idle_exit_after
    )
}

fn write_runtime_config<W: Write>(stdout: &mut W, config: &Config) -> io::Result<()> {
    writeln!(
        stdout,
        "runtime.socket_dir: {}",
        config.runtime.socket_dir.display()
    )
}

fn write_session_config<W: Write>(stdout: &mut W, config: &Config) -> io::Result<()> {
    writeln!(
        stdout,
        "session.new_session_on_ssh: {}",
        config.session.new_session_on_ssh
    )?;
    writeln!(
        stdout,
        "session.default_shell: {}",
        config.session.default_shell
    )?;
    writeln!(stdout, "session.kill_grace: {}", config.session.kill_grace)
}

fn write_ring_buffer_config<W: Write>(stdout: &mut W, config: &Config) -> io::Result<()> {
    writeln!(
        stdout,
        "ring_buffer.default_size: {}",
        config.ring_buffer.default_size
    )?;
    writeln!(
        stdout,
        "ring_buffer.max_size: {}",
        config.ring_buffer.max_size
    )?;
    writeln!(
        stdout,
        "ring_buffer.replay_on_attach: {}",
        config.ring_buffer.replay_on_attach
    )?;
    writeln!(
        stdout,
        "ring_buffer.replay_bytes: {}",
        config.ring_buffer.replay_bytes
    )
}

fn write_logging_config<W: Write>(stdout: &mut W, config: &Config) -> io::Result<()> {
    writeln!(
        stdout,
        "logging.session_log: {}",
        config.logging.session_log
    )?;
    writeln!(
        stdout,
        "logging.max_file_size: {}",
        config.logging.max_file_size
    )?;
    writeln!(stdout, "logging.max_files: {}", config.logging.max_files)?;
    writeln!(
        stdout,
        "logging.retention_days: {}",
        config.logging.retention_days
    )?;
    writeln!(
        stdout,
        "logging.flush_interval: {}",
        config.logging.flush_interval
    )
}

fn write_internal_log_config<W: Write>(stdout: &mut W, config: &Config) -> io::Result<()> {
    writeln!(stdout, "internal_log.level: {}", config.internal_log.level)?;
    writeln!(
        stdout,
        "internal_log.daemon_log: {}",
        config.internal_log.daemon_log.display()
    )?;
    writeln!(
        stdout,
        "internal_log.client_log: {}",
        config.internal_log.client_log.display()
    )?;
    writeln!(
        stdout,
        "internal_log.max_file_size: {}",
        config.internal_log.max_file_size
    )?;
    writeln!(
        stdout,
        "internal_log.max_files: {}",
        config.internal_log.max_files
    )
}

fn write_security_config<W: Write>(stdout: &mut W, config: &Config) -> io::Result<()> {
    writeln!(
        stdout,
        "security.allow_root_attach_others: {}",
        config.security.allow_root_attach_others
    )?;
    writeln!(
        stdout,
        "security.enable_input_recording: {}",
        config.security.enable_input_recording
    )
}

fn write_ssh_config<W: Write>(stdout: &mut W, config: &Config) -> io::Result<()> {
    writeln!(stdout, "ssh.auto_hook: {}", config.ssh.auto_hook)?;
    writeln!(stdout, "ssh.bypass_env: {}", config.ssh.bypass_env)
}

fn write_doctor<W: Write>(
    stdout: &mut W,
    loaded_config: Option<&Config>,
) -> Result<(), PersistError> {
    let fallback_config;
    let config = if let Some(config) = loaded_config {
        config
    } else {
        fallback_config = persist_core::load_default_config()?;
        &fallback_config
    };

    writeln!(stdout, "PersistShell doctor")
        .and_then(|_| writeln!(stdout, "status: engineering skeleton"))
        .and_then(|_| writeln!(stdout, "config: valid"))
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
        .and_then(|_| {
            writeln!(
                stdout,
                "client_log: {}",
                config.internal_log.client_log.display()
            )
        })
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

    #[test]
    fn config_output_contains_effective_values() {
        let paths = persist_core::ConfigPaths::from_base_dirs(
            "/home/alice".into(),
            None,
            None,
            None,
            "/run/user/1000".into(),
        );
        let options = ConfigLoadOptions::from_paths(
            paths.clone(),
            "/etc/persistshell/config.toml".into(),
            paths.config_dir.join("config.toml"),
        );
        let config = Config::default_with_paths(paths);
        let mut out = Vec::new();

        write_config_values(&mut out, &options, &config).expect("write config");

        let output = String::from_utf8(out).expect("utf8");
        assert!(output.contains("daemon.auto_start: true"));
        assert!(output.contains("ring_buffer.default_size: 8MB"));
        assert!(output.contains("internal_log.level: info"));
        assert!(output.contains("ssh.bypass_env: PERSIST_DISABLE"));
    }
}
