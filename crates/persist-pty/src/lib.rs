pub mod platform;
pub mod process;
pub mod pty;
pub mod signal;
pub mod termios;

use std::ffi::CString;
use std::io::{self, Write};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::io::RawFd;
use std::path::Path;
use std::time::Duration;

use persist_core::{PersistError, Result};

use crate::pty::{detect_shell, open_pty, set_nonblocking};

#[derive(Debug)]
pub struct PtyEngine;

impl PtyEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn open_session(&self) -> Result<PtySession> {
        let shell = detect_shell();
        self.open_session_with_shell(&shell, None)
    }

    pub fn open_session_with_shell(
        &self,
        shell: &str,
        histfile: Option<&str>,
    ) -> Result<PtySession> {
        self.open_session_with_context(shell, histfile, None, &[])
    }

    pub fn open_session_with_context(
        &self,
        shell: &str,
        histfile: Option<&str>,
        cwd: Option<&Path>,
        environment: &[(String, String)],
    ) -> Result<PtySession> {
        let (master_fd, slave_path) = open_pty()?;

        let slave_cstr = CString::new(
            slave_path
                .to_str()
                .ok_or_else(|| PersistError::invalid_argument("slave path is not valid UTF-8"))?,
        )
        .map_err(|_| PersistError::invalid_argument("slave path contains null bytes"))?;

        let shell_cstr = CString::new(shell)
            .map_err(|_| PersistError::invalid_argument("shell path contains null bytes"))?;

        let histfile_cstr = histfile
            .map(|h| {
                CString::new(h).map_err(|_| {
                    PersistError::invalid_argument("histfile path contains null bytes")
                })
            })
            .transpose()?;

        let cwd_cstr = cwd
            .map(|path| {
                CString::new(path.to_string_lossy().as_bytes())
                    .map_err(|_| PersistError::invalid_argument("cwd path contains null bytes"))
            })
            .transpose()?;
        let environment_cstr = environment
            .iter()
            .map(|(name, value)| {
                if name.is_empty() || name.contains('=') {
                    return Err(PersistError::invalid_argument(
                        "environment variable name is invalid",
                    ));
                }
                Ok((
                    CString::new(name.as_str()).map_err(|_| {
                        PersistError::invalid_argument(
                            "environment variable name contains null bytes",
                        )
                    })?,
                    CString::new(value.as_str()).map_err(|_| {
                        PersistError::invalid_argument(
                            "environment variable value contains null bytes",
                        )
                    })?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let agent_socket = std::env::var_os("SSH_AUTH_SOCK")
            .filter(|path| is_valid_agent_socket(Path::new(path)))
            .map(|path| {
                CString::new(path.as_encoded_bytes()).map_err(|_| {
                    PersistError::invalid_argument("SSH_AUTH_SOCK contains null bytes")
                })
            })
            .transpose()?;

        match unsafe { libc::fork() } {
            -1 => {
                let err = std::io::Error::last_os_error();
                unsafe { libc::close(master_fd) };
                Err(PersistError::Io {
                    operation: "fork",
                    source: err,
                })
            }
            0 => {
                child_setup(
                    slave_cstr,
                    master_fd,
                    shell_cstr,
                    histfile_cstr,
                    cwd_cstr,
                    environment_cstr,
                    agent_socket,
                );
                unsafe { libc::_exit(127) };
            }
            pid => {
                set_nonblocking(master_fd)?;
                Ok(PtySession {
                    master_fd,
                    child_pid: pid,
                    shell: shell.to_string(),
                    exit_status: None,
                })
            }
        }
    }
}

impl Default for PtyEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn is_valid_agent_socket(path: &Path) -> bool {
    path.is_absolute()
        && std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_socket())
}

fn child_setup(
    slave_cstr: CString,
    master_fd: RawFd,
    shell_cstr: CString,
    histfile: Option<CString>,
    cwd: Option<CString>,
    environment: Vec<(CString, CString)>,
    agent_socket: Option<CString>,
) {
    unsafe {
        libc::setsid();

        let slave_fd = libc::open(slave_cstr.as_ptr(), libc::O_RDWR);
        if slave_fd < 0 {
            libc::_exit(126);
        }

        libc::ioctl(slave_fd, libc::TIOCSCTTY, 0);

        libc::dup2(slave_fd, libc::STDIN_FILENO);
        libc::dup2(slave_fd, libc::STDOUT_FILENO);
        libc::dup2(slave_fd, libc::STDERR_FILENO);

        if slave_fd > 2 {
            libc::close(slave_fd);
        }
        libc::close(master_fd);

        if let Some(ref cwd) = cwd {
            libc::chdir(cwd.as_ptr());
        }

        for (name, value) in environment {
            libc::setenv(name.as_ptr(), value.as_ptr(), 1);
        }

        let agent_name = CString::new("SSH_AUTH_SOCK").expect("agent variable name is valid");
        if let Some(agent_socket) = agent_socket {
            libc::setenv(agent_name.as_ptr(), agent_socket.as_ptr(), 1);
        } else {
            libc::unsetenv(agent_name.as_ptr());
        }

        if let Some(ref hf) = histfile {
            let hf_name = CString::new("HISTFILE").expect("HISTFILE is not null");
            libc::setenv(hf_name.as_ptr(), hf.as_ptr(), 1);
        }

        let args = [shell_cstr.as_ptr(), std::ptr::null()];
        libc::execvp(shell_cstr.as_ptr(), args.as_ptr());
    }
}

#[derive(Debug)]
pub struct PtySession {
    master_fd: RawFd,
    child_pid: libc::pid_t,
    shell: String,
    exit_status: Option<i32>,
}

impl PtySession {
    pub fn read_output(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = unsafe {
            libc::read(
                self.master_fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
            )
        };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock {
                return Ok(0);
            }
            return Err(err);
        }
        Ok(n as usize)
    }

    pub fn write_input(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = unsafe {
            libc::write(
                self.master_fd,
                buf.as_ptr() as *const libc::c_void,
                buf.len(),
            )
        };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(n as usize)
    }

    pub fn master_fd(&self) -> RawFd {
        self.master_fd
    }

    pub fn foreground_process_group(&self) -> Option<u32> {
        let pgid = unsafe { libc::tcgetpgrp(self.master_fd) };
        (pgid > 0).then_some(pgid as u32)
    }

    pub fn child_pid(&self) -> u32 {
        self.child_pid as u32
    }

    pub fn shell(&self) -> &str {
        &self.shell
    }

    pub fn signal_child(&self, sig: i32) -> io::Result<()> {
        let ret = unsafe { libc::kill(self.child_pid, sig) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_status
    }

    pub fn is_alive(&self) -> bool {
        self.exit_status.is_none()
            && std::path::Path::new(&format!("/proc/{}", self.child_pid)).exists()
    }

    pub fn poll_output(&self, timeout: Duration) -> io::Result<bool> {
        let mut pfd = libc::pollfd {
            fd: self.master_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as i32) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(result > 0)
    }

    pub fn wait_exit(&mut self) -> Result<i32> {
        if let Some(status) = self.exit_status {
            return Ok(status);
        }

        let mut wstatus: i32 = 0;
        let result = unsafe { libc::waitpid(self.child_pid, &mut wstatus, 0) };
        if result < 0 {
            return Err(PersistError::Io {
                operation: "waitpid",
                source: io::Error::last_os_error(),
            });
        }

        if libc::WIFEXITED(wstatus) {
            let code = libc::WEXITSTATUS(wstatus);
            self.exit_status = Some(code);
            Ok(code)
        } else if libc::WIFSIGNALED(wstatus) {
            let sig = libc::WTERMSIG(wstatus);
            self.exit_status = Some(128 + sig);
            Ok(128 + sig)
        } else {
            Err(PersistError::internal_error(format!(
                "child process exited with unknown status: {wstatus}"
            )))
        }
    }

    pub fn poll_exit(&mut self) -> Result<Option<i32>> {
        if let Some(status) = self.exit_status {
            return Ok(Some(status));
        }

        let mut wstatus: i32 = 0;
        let result = unsafe { libc::waitpid(self.child_pid, &mut wstatus, libc::WNOHANG) };
        if result == 0 {
            return Ok(None);
        }
        if result < 0 {
            return Err(PersistError::Io {
                operation: "waitpid nonblocking",
                source: io::Error::last_os_error(),
            });
        }
        let status = if libc::WIFEXITED(wstatus) {
            libc::WEXITSTATUS(wstatus)
        } else if libc::WIFSIGNALED(wstatus) {
            128 + libc::WTERMSIG(wstatus)
        } else {
            return Err(PersistError::internal_error(format!(
                "child process exited with unknown status: {wstatus}"
            )));
        };
        self.exit_status = Some(status);
        Ok(Some(status))
    }

    fn cleanup_child(&mut self) {
        if !self.is_alive() {
            return;
        }
        unsafe {
            libc::kill(self.child_pid, libc::SIGHUP);
        }

        for _ in 0..30 {
            let mut wstatus: i32 = 0;
            let result = unsafe { libc::waitpid(self.child_pid, &mut wstatus, libc::WNOHANG) };
            if result == self.child_pid {
                if libc::WIFEXITED(wstatus) {
                    self.exit_status = Some(libc::WEXITSTATUS(wstatus));
                } else if libc::WIFSIGNALED(wstatus) {
                    let sig = libc::WTERMSIG(wstatus);
                    self.exit_status = Some(128 + sig);
                }
                return;
            }
            if result < 0 {
                // Child may have been reaped already
                self.exit_status = Some(-1);
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // Force kill
        unsafe {
            libc::kill(self.child_pid, libc::SIGKILL);
            libc::waitpid(self.child_pid, std::ptr::null_mut(), 0);
        }
        self.exit_status = Some(-1);
    }
}

impl Write for PtySession {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_input(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.cleanup_child();
        unsafe {
            libc::close(self.master_fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    use std::time::Duration;

    #[test]
    fn agent_socket_validation_requires_unix_socket() {
        let dir = std::env::temp_dir().join(format!(
            "persist-agent-socket-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create dir");
        let socket_path = dir.join("agent.sock");
        let file_path = dir.join("not-a-socket");
        let listener = UnixListener::bind(&socket_path).expect("bind socket");
        std::fs::write(&file_path, b"not a socket").expect("write file");

        assert!(is_valid_agent_socket(&socket_path));
        assert!(!is_valid_agent_socket(&file_path));

        drop(listener);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pty_histfile_env_set() {
        let engine = PtyEngine::new();
        let histfile = "/tmp/persist-test-history";
        let mut session = engine
            .open_session_with_shell("/bin/sh", Some(histfile))
            .expect("open session");
        std::thread::sleep(Duration::from_millis(200));
        writeln!(session, "echo $HISTFILE").expect("write");
        let mut buf = vec![0u8; 4096];
        let mut output = String::new();
        for _ in 0..50 {
            if session
                .poll_output(Duration::from_millis(100))
                .unwrap_or(false)
            {
                let n = session.read_output(&mut buf).unwrap_or(0);
                if n > 0 {
                    output.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if output.contains("/tmp/persist-test-history") {
                        break;
                    }
                }
            } else {
                break;
            }
        }
        assert!(
            output.contains("/tmp/persist-test-history"),
            "HISTFILE should be set, got: {output:?}"
        );
    }

    #[test]
    fn pty_context_sets_cwd_and_environment() {
        let engine = PtyEngine::new();
        let mut session = engine
            .open_session_with_context(
                "/bin/sh",
                None,
                Some(Path::new("/")),
                &[("PERSIST_TEST_CONTEXT".into(), "restored".into())],
            )
            .expect("open session");
        std::thread::sleep(Duration::from_millis(200));
        writeln!(
            session,
            "printf '%s:%s\\n' \"$PWD\" \"$PERSIST_TEST_CONTEXT\""
        )
        .expect("write");

        let mut buf = vec![0u8; 4096];
        let mut output = String::new();
        for _ in 0..50 {
            if session
                .poll_output(Duration::from_millis(100))
                .unwrap_or(false)
            {
                let n = session.read_output(&mut buf).unwrap_or(0);
                output.push_str(&String::from_utf8_lossy(&buf[..n]));
                if output.contains("/:restored") {
                    break;
                }
            }
        }
        assert!(
            output.contains("/:restored"),
            "unexpected output: {output:?}"
        );
    }

    #[test]
    fn pty_reports_foreground_process_group() {
        let engine = PtyEngine::new();
        let session = engine
            .open_session_with_shell("/bin/sh", None)
            .expect("open session");
        std::thread::sleep(Duration::from_millis(100));
        assert!(session.foreground_process_group().is_some());
    }

    #[test]
    fn pty_echo_works() {
        let engine = PtyEngine::new();
        let mut session = engine.open_session().expect("open session");

        // Wait for shell to be ready
        std::thread::sleep(Duration::from_millis(200));

        // Send a command
        writeln!(session, "echo hello_pty_test").expect("write");

        // Read output
        let mut buf = vec![0u8; 4096];
        let mut output = String::new();
        for _ in 0..50 {
            if session
                .poll_output(Duration::from_millis(100))
                .unwrap_or(false)
            {
                let n = session.read_output(&mut buf).unwrap_or(0);
                if n > 0 {
                    output.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if output.contains("hello_pty_test") {
                        break;
                    }
                }
            } else {
                break;
            }
        }

        assert!(
            output.contains("hello_pty_test"),
            "expected output to contain hello_pty_test, got: {output:?}"
        );
    }

    #[test]
    fn pty_exit_code() {
        let engine = PtyEngine::new();
        let mut session = engine.open_session().expect("open session");

        std::thread::sleep(Duration::from_millis(200));

        // Send exit command
        writeln!(session, "exit 42").expect("write exit");

        let code = session.wait_exit().expect("wait exit");
        assert_eq!(code, 42);
    }

    #[test]
    fn pty_poll_exit_reaps_terminated_child() {
        let engine = PtyEngine::new();
        let mut session = engine.open_session().expect("open session");
        writeln!(session, "exit 17").expect("write exit");

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(code) = session.poll_exit().expect("poll exit") {
                assert_eq!(code, 17);
                break;
            }
            assert!(std::time::Instant::now() < deadline, "child did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn pty_drop_cleans_up_child() {
        let pid;
        {
            let engine = PtyEngine::new();
            let session = engine.open_session().expect("open session");
            pid = session.child_pid();
            assert!(
                std::path::Path::new(&format!("/proc/{pid}")).exists(),
                "child should be alive after creation"
            );
            // Let session go out of scope, drop is called
        }

        // Give it a moment to clean up
        std::thread::sleep(Duration::from_millis(300));

        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists() || {
                // Zygote check: process could be an orphaned zombie
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok();
                stat.map_or(true, |s| s.contains("Z)") || s.contains("X)"))
            },
            "child should be cleaned up after PtySession drop"
        );
    }

    #[test]
    fn pty_is_alive_returns_correct_state() {
        let engine = PtyEngine::new();
        let mut session = engine.open_session().expect("open session");

        assert!(session.is_alive(), "session should be alive initially");

        writeln!(session, "exit").expect("write exit");
        session.wait_exit().expect("wait exit");

        assert!(
            !session.is_alive(),
            "session should not be alive after exit"
        );
    }

    #[test]
    fn pty_write_and_read_large_output() {
        let engine = PtyEngine::new();
        let mut session = engine.open_session().expect("open session");

        std::thread::sleep(Duration::from_millis(200));

        // Generate large output
        writeln!(session, "for i in $(seq 1 100); do echo \"line $i\"; done").expect("write");

        let mut buf = vec![0u8; 4096];
        let mut total = String::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            if session
                .poll_output(Duration::from_millis(100))
                .unwrap_or(false)
            {
                let n = session.read_output(&mut buf).unwrap_or(0);
                if n > 0 {
                    total.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if total.contains("line 100") {
                        break;
                    }
                }
            }
        }

        assert!(
            total.contains("line 100"),
            "should read large output, got {} bytes: {total:?}",
            total.len()
        );
    }

    #[test]
    fn pty_wait_exit_returns_signal_exit_code() {
        // Using SIGKILL equivalent: sending kill signal to self
        let engine = PtyEngine::new();
        let mut session = engine.open_session().expect("open session");

        std::thread::sleep(Duration::from_millis(200));

        writeln!(session, "kill -9 $$").expect("write kill");

        let code = session.wait_exit().expect("wait exit");
        // Killed by SIGKILL (9): exit code = 128 + 9 = 137
        assert_eq!(code, 137, "SIGKILL should produce exit code 137");
    }
}
