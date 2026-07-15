use std::io::{self, Write};
use std::process::ExitCode;

use std::path::Path;

use persist_core::{
    init_logging, load_config, log_message, version_string, Config, ConfigLoadOptions, LogLevel,
    LoggerConfig, PersistError,
};
use persist_ipc::{
    encode_hello, read_frame, write_frame, ClientSocket, Frame, HelloPayload, MessageType,
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
            let _ = writeln!(stderr, "{}", error.user_facing("persist"));
            error.exit_code()
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
        Command::Doctor
            | Command::Config
            | Command::Daemon { .. }
            | Command::Attach { .. }
            | Command::New
            | Command::List { .. }
            | Command::ProcessTree { .. }
            | Command::ProcessStats { .. }
            | Command::Snapshot { .. }
            | Command::Metrics
            | Command::Close { .. }
            | Command::Kill { .. }
            | Command::Log { .. }
            | Command::LogSearch { .. }
            | Command::Rename { .. }
            | Command::Detach { .. }
            | Command::Note { .. }
            | Command::Tag { .. }
            | Command::Pin { .. }
            | Command::Lock { .. }
            | Command::LogExport { .. }
            | Command::Replay { .. }
            | Command::Install
            | Command::Uninstall { .. }
    )
}

fn command_log_message(command: &Command) -> &'static str {
    match command {
        Command::Help => "command=help started",
        Command::Version => "command=version started",
        Command::Doctor => "command=doctor started",
        Command::Config => "command=config started",
        Command::Daemon { .. } => "command=daemon started",
        Command::Attach { .. } => "command=attach started",
        Command::New => "command=new started",
        Command::List { .. } => "command=ls started",
        Command::ProcessTree { .. } => "command=ps started",
        Command::ProcessStats { .. } => "command=stats started",
        Command::Snapshot { .. } => "command=snapshot started",
        Command::Metrics => "command=metrics started",
        Command::Close { .. } => "command=close started",
        Command::Kill { .. } => "command=kill started",
        Command::Log { .. } => "command=log started",
        Command::LogSearch { .. } => "command=log_search started",
        Command::Install => "command=install started",
        Command::Uninstall { .. } => "command=uninstall started",
        Command::Rename { .. } => "command=rename started",
        Command::Detach { .. } => "command=detach started",
        Command::Note { .. } => "command=note started",
        Command::Tag { .. } => "command=tag started",
        Command::Pin { .. } => "command=pin started",
        Command::Lock { .. } => "command=lock started",
        Command::LogExport { .. } => "command=log_export started",
        Command::Replay { .. } => "command=replay started",
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
        Command::Daemon { action } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            match action.as_deref() {
                Some("start") | None => crate::daemon::daemon_start(config),
                Some("stop") => crate::daemon::daemon_stop(config),
                Some("status") => crate::daemon::daemon_status(config, stdout),
                Some(other) => Err(PersistError::invalid_argument(format!(
                    "unknown daemon action: {other}"
                ))),
            }
        }
        Command::Attach {
            session_id,
            readonly,
        } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::attach::attach(config, session_id, readonly)
        }
        Command::New => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::new_session(config)
        }
        Command::List { tag_filter } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::list_sessions(config, tag_filter.as_deref())
        }
        Command::ProcessTree { session_id } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::process_tree(config, session_id)
        }
        Command::ProcessStats { session_id } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::process_stats(config, session_id)
        }
        Command::Snapshot { session_id } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::snapshot(config, session_id)
        }
        Command::Metrics => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::metrics(config)
        }
        Command::Close { session_id } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::close_session(config, session_id)
        }
        Command::Note { session_id, text } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::note_session(config, session_id, text.as_deref())
        }
        Command::Tag {
            session_id,
            action,
            tag,
        } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::tag_session(config, session_id, &action, tag.as_deref())
        }
        Command::Kill { session_id } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::kill_session(config, session_id)
        }
        Command::Log { session_id } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::read_session_log(config, session_id, stdout)
        }
        Command::LogSearch {
            keyword,
            session_id,
            case_insensitive,
        } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::log_search(config, &keyword, session_id, case_insensitive, stdout)
        }
        Command::LogExport {
            session_id,
            output_path,
        } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::export_session_log(config, session_id, output_path.as_deref(), stdout)
        }
        Command::Rename { session_id, name } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::rename_session(config, session_id, &name)
        }
        Command::Detach { session_id } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::signal_detach(config, session_id)
        }
        Command::Pin { session_id, pinned } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::pin_session(config, session_id, pinned)
        }
        Command::Lock { session_id, locked } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::lock_session(config, session_id, locked)
        }
        Command::Replay {
            session_id,
            tail,
            head,
            speed,
            follow,
        } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::session::replay_session(config, session_id, tail, head, speed, follow, stdout)
        }
        Command::Install => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::installer::install(config)
        }
        Command::Uninstall { purge } => {
            let config = loaded_config
                .map(|(_, c)| c)
                .ok_or_else(|| PersistError::internal_error("config not loaded"))?;
            crate::installer::uninstall(config, purge)
        }
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
  help          Show this help
  version       Show version information
  doctor        Show local skeleton diagnostics
  config        Show effective configuration
  daemon start  Start the persist daemon
  daemon stop   Stop the persist daemon
  daemon status Check daemon status
  new                    Create a new session
  ls [--tag <tag>]       List sessions, optionally filtered by tag
  ps <id>                Show the foreground process tree
  stats <id>             Show foreground process resource counters
  snapshot <id>          Show a bounded session JSON snapshot
  metrics                Show daemon and session metrics
  attach                 Attach to a session
  close <id>    Close a session gracefully
  kill <id>     Force kill a session
  log <id>              Show session output log
   log search <keyword> [--session <id>] [-i]  Search session logs
  rename <id> <name>    Rename a session
   note <id> [text]    Show or set a session note (empty text to clear)
    tag <id> <add|remove|list> [<tag>]  Manage session tags
    pin <id>    Pin (favorite) a session
   unpin <id>   Unpin a session
  detach <id>           Detach a session (disconnect the active client)
  replay <id> [--tail <n>] [--head <n>] [--speed <f>] [--follow]  Replay session history
  install       Install shell hook for SSH auto-attach
  uninstall     Remove shell hook (--purge to also delete all data)
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

    let mut ok = true;

    doctor_writeln(stdout, "PersistShell doctor")?;
    doctor_writeln(stdout, "")?;

    // ── Config ──
    doctor_writeln(stdout, "── Config ──")?;
    writeln!(stdout, "  socket: {}", config.paths.socket_path.display()).map_err(|e| {
        PersistError::Io {
            operation: "doctor write",
            source: e,
        }
    })?;

    if !doctor_check_config_sanity(config) {
        ok = false;
    }

    // ── Directories ──
    doctor_writeln(stdout, "")?;
    doctor_writeln(stdout, "── Directories ──")?;
    doctor_check_path(stdout, "config_dir", &config.paths.config_dir)?;
    if !doctor_check_dir_perms(stdout, "config_dir", &config.paths.config_dir, 0o755)? {
        ok = false;
    }
    doctor_check_path(stdout, "data_dir", &config.paths.data_dir)?;
    if !doctor_check_dir_perms(stdout, "data_dir", &config.paths.data_dir, 0o755)? {
        ok = false;
    }
    doctor_check_path(stdout, "state_dir", &config.paths.state_dir)?;
    if !doctor_check_dir_perms(stdout, "state_dir", &config.paths.state_dir, 0o755)? {
        ok = false;
    }
    doctor_check_path(stdout, "runtime_dir", &config.paths.runtime_dir)?;
    if !doctor_check_dir_perms(stdout, "runtime_dir", &config.paths.runtime_dir, 0o700)? {
        ok = false;
    }
    let sock_parent = config.paths.socket_path.parent().unwrap_or(Path::new("/"));
    doctor_check_path(stdout, "socket_dir", sock_parent)?;

    // ── System ──
    doctor_writeln(stdout, "")?;
    doctor_writeln(stdout, "── System ──")?;
    if !doctor_check_pty(stdout)? {
        ok = false;
    }
    if !doctor_check_shell_hook(stdout)? {
        ok = false;
    }

    // ── Daemon ──
    doctor_writeln(stdout, "")?;
    doctor_writeln(stdout, "── Daemon ──")?;
    if !doctor_check_daemon_health(stdout, config)? {
        ok = false;
    }

    // ── Socket Permissions ──
    doctor_writeln(stdout, "")?;
    doctor_writeln(stdout, "── Socket ──")?;
    if config.paths.socket_path.exists() {
        match std::fs::metadata(&config.paths.socket_path) {
            Ok(meta) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let perms = meta.permissions().mode() & 0o777;
                    if perms == 0o600 {
                        doctor_writeln(
                            stdout,
                            &format!(
                                "  {}: perms {:03o} ✓",
                                config.paths.socket_path.display(),
                                perms
                            ),
                        )?;
                    } else {
                        doctor_writeln(
                            stdout,
                            &format!(
                                "  {}: perms {:03o} (expected 0600) ⚠",
                                config.paths.socket_path.display(),
                                perms
                            ),
                        )?;
                        ok = false;
                    }
                }
            }
            Err(e) => {
                doctor_writeln(
                    stdout,
                    &format!(
                        "  {}: cannot read metadata: {}",
                        config.paths.socket_path.display(),
                        e
                    ),
                )?;
                ok = false;
            }
        }
    } else {
        doctor_writeln(
            stdout,
            &format!(
                "  {}: does not exist yet",
                config.paths.socket_path.display()
            ),
        )?;
    }

    // ── Client Log ──
    doctor_writeln(stdout, "")?;
    doctor_writeln(stdout, "── Logging ──")?;
    let client_log = &config.internal_log.client_log;
    if let Some(parent) = client_log.parent() {
        doctor_check_path(stdout, "client_log_dir", parent)?;
    }

    doctor_writeln(stdout, "")?;
    if ok {
        doctor_writeln(stdout, "All checks passed ✓")
    } else {
        doctor_writeln(stdout, "Some checks FAILED — see above for details")
    }
}

fn doctor_check_config_sanity(config: &Config) -> bool {
    let mut ok = true;
    if config.daemon.gc_idle_timeout.duration() == std::time::Duration::ZERO
        && config.daemon.idle_exit
    {
        ok = true;
    }
    ok
}

fn doctor_check_dir_perms<W: Write>(
    stdout: &mut W,
    label: &str,
    path: &Path,
    expected: u32,
) -> std::result::Result<bool, PersistError> {
    if !path.exists() {
        return Ok(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(meta) => {
                let perms = meta.permissions().mode() & 0o777;
                if perms == expected {
                    doctor_writeln(stdout, &format!("  {} perms: {:03o} ✓", label, perms))?;
                    Ok(true)
                } else {
                    doctor_writeln(
                        stdout,
                        &format!(
                            "  {} perms: {:03o} (expected {:03o}) ⚠",
                            label, perms, expected
                        ),
                    )?;
                    Ok(false)
                }
            }
            Err(e) => {
                doctor_writeln(stdout, &format!("  {} perms: cannot read: {}", label, e))?;
                Ok(false)
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (stdout, label, path, expected);
        Ok(true)
    }
}

fn doctor_check_pty<W: Write>(stdout: &mut W) -> std::result::Result<bool, PersistError> {
    #[cfg(unix)]
    {
        match unsafe {
            libc::openpty(
                &mut 0,
                &mut 0,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        } {
            0 => {
                doctor_writeln(stdout, "  pty: available ✓")?;
                Ok(true)
            }
            _ => {
                doctor_writeln(
                    stdout,
                    &format!("  pty: unavailable ({}) ⚠", std::io::Error::last_os_error()),
                )?;
                Ok(false)
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = stdout;
        doctor_writeln(stdout, "  pty: not available on this platform")?;
        Ok(false)
    }
}

fn doctor_check_shell_hook<W: Write>(stdout: &mut W) -> std::result::Result<bool, PersistError> {
    let marker = "# === PERSISTSHELL AUTO-HOOK ===";
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let shell = std::env::var("SHELL").ok();
    let candidates: Vec<std::path::PathBuf> = match (&home, &shell) {
        (Some(home), Some(shell)) if shell.ends_with("/zsh") => {
            vec![home.join(".zshrc"), home.join(".zprofile")]
        }
        (Some(home), Some(shell)) if shell.ends_with("/bash") => {
            vec![
                home.join(".bashrc"),
                home.join(".bash_profile"),
                home.join(".profile"),
            ]
        }
        (Some(home), _) => {
            vec![
                home.join(".bashrc"),
                home.join(".zshrc"),
                home.join(".profile"),
            ]
        }
        _ => Vec::new(),
    };
    for candidate in &candidates {
        if let Ok(content) = std::fs::read_to_string(candidate) {
            if content.contains(marker) {
                doctor_writeln(
                    stdout,
                    &format!("  shell hook: found in {} ✓", candidate.display()),
                )?;
                return Ok(true);
            }
        }
    }
    doctor_writeln(
        stdout,
        "  shell hook: NOT installed (run `persist install`) ⚠",
    )?;
    Ok(false)
}

fn doctor_check_daemon_health<W: Write>(
    stdout: &mut W,
    config: &Config,
) -> std::result::Result<bool, PersistError> {
    let socket_path = &config.paths.socket_path;
    if !socket_path.exists() {
        doctor_writeln(stdout, "  daemon: NOT running (socket file not found)")?;
        return Ok(false);
    }
    match ClientSocket::connect(socket_path) {
        Ok(mut sock) => {
            let payload = encode_hello(&HelloPayload {
                protocol_major: 0,
                protocol_minor: 1,
                uid: unsafe { libc::getuid() },
                pid: std::process::id(),
            });
            if let Err(e) = write_frame(
                sock.stream(),
                &Frame {
                    msg_type: MessageType::Hello,
                    flags: 0,
                    request_id: 0,
                    payload,
                },
            ) {
                doctor_writeln(stdout, &format!("  daemon: HELLO failed ({}) ⚠", e))?;
                return Ok(false);
            }
            match read_frame(sock.stream()) {
                Ok(ack) if ack.msg_type == MessageType::HelloAck => {
                    doctor_writeln(stdout, "  daemon: running ✓")?;
                    Ok(true)
                }
                Ok(_) => {
                    doctor_writeln(stdout, "  daemon: unexpected response ⚠")?;
                    Ok(false)
                }
                Err(e) => {
                    doctor_writeln(stdout, &format!("  daemon: read failed ({}) ⚠", e))?;
                    Ok(false)
                }
            }
        }
        Err(e) => {
            doctor_writeln(stdout, &format!("  daemon: connect failed ({}) ⚠", e))?;
            Ok(false)
        }
    }
}

fn doctor_writeln<W: Write>(stdout: &mut W, s: &str) -> std::result::Result<(), PersistError> {
    writeln!(stdout, "{}", s).map_err(|e| PersistError::Io {
        operation: "doctor write",
        source: e,
    })
}

fn doctor_check_path<W: Write>(
    stdout: &mut W,
    label: &str,
    path: &Path,
) -> std::result::Result<(), PersistError> {
    if path.exists() {
        doctor_writeln(stdout, &format!("  {}: {} ✓", label, path.display()))
    } else {
        doctor_writeln(
            stdout,
            &format!(
                "  {}: {} MISSING (will be created on first use)",
                label,
                path.display()
            ),
        )
    }
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

        assert_eq!(code, 1);
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

    #[test]
    fn doctor_config_sanity_allows_zero_gc_timeout() {
        let paths = persist_core::ConfigPaths::from_base_dirs(
            "/home/alice".into(),
            None,
            None,
            None,
            "/run/user/1000".into(),
        );
        let config = Config::default_with_paths(paths);
        assert!(doctor_check_config_sanity(&config));
    }

    #[test]
    fn doctor_check_path_exists_or_missing() {
        let dir = std::env::temp_dir().join("persist-doctor-test-check-path");
        let _ = std::fs::create_dir_all(&dir);
        let mut out = Vec::new();
        doctor_check_path(&mut out, "test_dir", &dir).expect("check path");
        let output = String::from_utf8(out.clone()).expect("utf8");
        assert!(output.contains("test_dir"));
        assert!(output.contains("✓"));

        let missing = dir.join("nonexistent");
        let mut out2 = Vec::new();
        doctor_check_path(&mut out2, "missing_dir", &missing).expect("check missing");
        let output2 = String::from_utf8(out2).expect("utf8");
        assert!(output2.contains("MISSING"));
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn doctor_dir_perms_nonexistent_returns_ok() {
        let dir = std::env::temp_dir().join("persist-doctor-test-perms-nonexistent");
        let _ = std::fs::create_dir_all(&dir);
        let missing = dir.join("does_not_exist");
        let mut out = Vec::new();
        let result = doctor_check_dir_perms(&mut out, "test", &missing, 0o755)
            .expect("no error for missing dir");
        assert!(result, "missing dir should not fail");
        let _ = std::fs::remove_dir(&dir);
    }
}
