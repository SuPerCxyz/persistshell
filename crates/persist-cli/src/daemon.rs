use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use persist_core::pidfile;
use persist_core::{Config, PersistError, Result};
use persist_ipc::{
    decode_note_get_resp, read_frame, write_frame, ClientSocket, Frame, MessageType,
};

pub fn daemon_start(config: &Config) -> Result<()> {
    let socket_path = &config.paths.socket_path;
    let runtime_dir = &config.paths.runtime_dir;

    fs::create_dir_all(runtime_dir).map_err(|source| PersistError::Io {
        operation: "create daemon runtime directory",
        source,
    })?;
    fs::set_permissions(runtime_dir, fs::Permissions::from_mode(0o700)).map_err(|source| {
        PersistError::Io {
            operation: "set daemon runtime directory permission",
            source,
        }
    })?;

    if socket_path.exists() {
        if daemon_socket_is_listening(socket_path) {
            return Err(PersistError::daemon_already_running());
        }
        cleanup_stale_socket(socket_path)?;
    }

    let pid_path = runtime_dir.join("daemon.pid");
    if pidfile::is_running(&pid_path) {
        return Err(PersistError::daemon_already_running());
    }

    let daemon_path = find_daemon_binary()?;
    let daemon_log = runtime_dir.join("daemon.log");

    let daemon_log_file = create_daemon_log(&daemon_log)?;
    let child = Command::new(&daemon_path)
        .arg("foreground")
        .stdout(
            daemon_log_file
                .try_clone()
                .map_err(|source| PersistError::Io {
                    operation: "clone daemon log file",
                    source,
                })?,
        )
        .stderr(daemon_log_file)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|source| PersistError::Io {
            operation: "spawn persistd",
            source,
        })?;

    let child_pid = child.id();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut started = false;
    while std::time::Instant::now() < deadline {
        if socket_path.exists() {
            started = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    if started {
        println!("daemon started (pid {child_pid})");
        Ok(())
    } else {
        let log_content = fs::read_to_string(&daemon_log).unwrap_or_default();
        let _ = child.wait_with_output();
        Err(PersistError::internal_error(format!(
            "daemon failed to start within 2s\n{}",
            if log_content.is_empty() {
                String::new()
            } else {
                format!("daemon log:\n{log_content}")
            }
        )))
    }
}

fn create_daemon_log(path: &Path) -> Result<std::fs::File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|source| PersistError::Io {
            operation: "create daemon log file",
            source,
        })?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        PersistError::Io {
            operation: "set daemon log permission",
            source,
        }
    })?;
    Ok(file)
}

pub fn daemon_stop(config: &Config) -> Result<()> {
    let pid_path = config.paths.runtime_dir.join("daemon.pid");

    let pid = pidfile::read_pid(&pid_path).ok_or_else(PersistError::daemon_not_running)?;

    if !pidfile::is_process_alive(pid) {
        cleanup_stale_pid(&pid_path)?;
        cleanup_stale_socket(&config.paths.socket_path)?;
        return Err(PersistError::daemon_not_running());
    }

    pidfile::send_signal(pid, libc::SIGTERM).map_err(|source| PersistError::Io {
        operation: "kill daemon",
        source,
    })?;

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut forced = false;
    while std::time::Instant::now() < deadline {
        if !pidfile::is_process_alive(pid) {
            forced = false;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    if pidfile::is_process_alive(pid) {
        pidfile::send_signal(pid, libc::SIGKILL).map_err(|source| PersistError::Io {
            operation: "force kill daemon",
            source,
        })?;
        forced = true;
    }

    cleanup_stale_socket(&config.paths.socket_path)?;

    if forced {
        println!("daemon forcefully killed (pid {pid})");
    } else {
        println!("daemon stopped (pid {pid})");
    }
    Ok(())
}

pub fn daemon_status<W: Write>(config: &Config, stdout: &mut W) -> Result<()> {
    let pid_path = config.paths.runtime_dir.join("daemon.pid");

    let pid = match pidfile::read_pid(&pid_path) {
        Some(pid) => pid,
        None => {
            let _ = writeln!(stdout, "daemon: stopped");
            return Ok(());
        }
    };

    if !pidfile::is_process_alive(pid) {
        let _ = writeln!(stdout, "daemon: stopped (stale pidfile)");
        let _ = writeln!(stdout, "run 'persist daemon stop' to clean up");
        return Ok(());
    }

    let socket_status = if config.paths.socket_path.exists() {
        "listening"
    } else {
        "missing"
    };

    let uptime = read_process_uptime(pid);

    let _ = writeln!(stdout, "daemon: running");
    let _ = writeln!(stdout, "pid: {pid}");
    if let Some(uptime) = uptime {
        let _ = writeln!(stdout, "uptime: {uptime}");
    }
    let _ = writeln!(stdout, "socket: {}", config.paths.socket_path.display());
    let _ = writeln!(stdout, "socket_status: {socket_status}");
    match read_daemon_metrics(config) {
        Ok(metrics) => {
            let connected = metrics
                .pointer("/holder/connected")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let _ = writeln!(
                stdout,
                "holder_status: {}",
                if connected {
                    "connected"
                } else {
                    "disconnected"
                }
            );
            if let Some(holder_pid) = metrics
                .pointer("/holder/pid")
                .and_then(serde_json::Value::as_u64)
            {
                let _ = writeln!(stdout, "holder_pid: {holder_pid}");
            }
            if let Some(instance) = metrics
                .pointer("/holder/instance")
                .and_then(serde_json::Value::as_str)
            {
                let _ = writeln!(stdout, "holder_instance: {instance}");
            }
            if let Some(degraded) = metrics
                .pointer("/sessions/log_degraded")
                .and_then(serde_json::Value::as_u64)
            {
                let _ = writeln!(stdout, "log_degraded: {degraded}");
            }
            if let Some(lost) = metrics
                .pointer("/sessions/lost")
                .and_then(serde_json::Value::as_u64)
            {
                let _ = writeln!(stdout, "lost: {lost}");
            }
        }
        Err(error) => {
            let _ = writeln!(stdout, "holder_status: unavailable");
            let _ = writeln!(stdout, "metrics_error: {error}");
        }
    }

    Ok(())
}

fn read_daemon_metrics(config: &Config) -> Result<serde_json::Value> {
    let mut socket = ClientSocket::connect(&config.paths.socket_path)?;
    socket.send_hello(unsafe { libc::getuid() }, std::process::id())?;
    write_frame(
        socket.stream(),
        &Frame {
            msg_type: MessageType::Metrics,
            flags: 0,
            request_id: 1,
            payload: Vec::new(),
        },
    )?;
    let response = read_frame(socket.stream())?;
    if response.msg_type != MessageType::MetricsResp {
        return Err(PersistError::invalid_argument(
            "daemon returned an unexpected metrics response",
        ));
    }
    let json = decode_note_get_resp(&response.payload)
        .ok_or_else(|| PersistError::invalid_argument("daemon returned malformed metrics"))?;
    serde_json::from_str(&json)
        .map_err(|_| PersistError::invalid_argument("daemon returned invalid metrics JSON"))
}

fn find_daemon_binary() -> Result<PathBuf> {
    let current_exe = std::env::current_exe().map_err(|source| PersistError::Io {
        operation: "get current executable path",
        source,
    })?;

    let daemon_path = current_exe
        .parent()
        .ok_or_else(|| PersistError::internal_error("cannot determine binary directory"))?
        .join("persistd");

    if daemon_path.exists() {
        Ok(daemon_path)
    } else {
        which("persistd").ok_or_else(|| {
            PersistError::internal_error("persistd not found next to persist binary or in PATH")
        })
    }
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(name);
            if full.exists() {
                Some(full)
            } else {
                None
            }
        })
    })
}

fn daemon_socket_is_listening(path: &Path) -> bool {
    use std::os::unix::net::UnixStream;
    UnixStream::connect(path).is_ok()
}

fn cleanup_stale_socket(path: &Path) -> Result<()> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(|source| PersistError::Io {
            operation: "inspect stale socket",
            source,
        })?;
        if !metadata.file_type().is_socket() {
            return Err(PersistError::invalid_argument(format!(
                "refusing to remove non-socket path: {}",
                path.display()
            )));
        }
        fs::remove_file(path).map_err(|source| PersistError::Io {
            operation: "cleanup stale socket",
            source,
        })
    } else {
        Ok(())
    }
}

fn cleanup_stale_pid(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).map_err(|source| PersistError::Io {
            operation: "cleanup stale pidfile",
            source,
        })
    } else {
        Ok(())
    }
}

fn read_process_uptime(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<&str> = stat.split_whitespace().collect();
    if fields.len() < 22 {
        return None;
    }
    let start_ticks: u64 = fields.get(21)?.parse().ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let boot_time = read_boot_time()?;
    let uptime_secs = now
        .checked_sub(boot_time)
        .and_then(|d| {
            let start_secs = start_ticks / 100;
            d.checked_sub(Duration::from_secs(start_secs))
        })?
        .as_secs();

    let mins = uptime_secs / 60;
    let secs = uptime_secs % 60;
    if mins >= 60 {
        let hours = mins / 60;
        let rem_mins = mins % 60;
        Some(format!("{hours}h {rem_mins}m {secs}s"))
    } else {
        Some(format!("{mins}m {secs}s"))
    }
}

fn read_boot_time() -> Option<Duration> {
    let content = fs::read_to_string("/proc/stat").ok()?;
    for line in content.lines() {
        if let Some(boot_str) = line.strip_prefix("btime ") {
            let boot_secs: u64 = boot_str.trim().parse().ok()?;
            return Some(Duration::from_secs(boot_secs));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_daemon_log_enforces_private_mode() {
        let path = std::env::temp_dir().join(format!(
            "persist-daemon-log-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::write(&path, b"old log").expect("write old log");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("set insecure mode");

        drop(create_daemon_log(&path).expect("create daemon log"));
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_file(path);
    }
}
