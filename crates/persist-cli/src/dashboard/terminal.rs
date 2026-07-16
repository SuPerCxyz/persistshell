use std::io::{self, Stdout};

use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use persist_core::{PersistError, Result};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

pub(super) struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    raw_enabled: bool,
    alternate_screen: bool,
}

impl TerminalGuard {
    pub(super) fn enter() -> Result<Self> {
        enable_raw_mode().map_err(|source| terminal_error("enable raw mode", source))?;
        let mut stdout = io::stdout();
        if let Err(source) = execute!(stdout, EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(terminal_error("enter alternate screen", source));
        }
        let terminal = match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => terminal,
            Err(source) => {
                let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
                let _ = disable_raw_mode();
                return Err(terminal_error("create dashboard terminal", source));
            }
        };
        Ok(Self {
            terminal,
            raw_enabled: true,
            alternate_screen: true,
        })
    }

    pub(super) fn draw(&mut self, render: impl FnOnce(&mut ratatui::Frame<'_>)) -> Result<()> {
        self.terminal
            .draw(render)
            .map(|_| ())
            .map_err(|source| terminal_error("draw dashboard", source))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.alternate_screen {
            let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen, Show);
            self.alternate_screen = false;
        }
        if self.raw_enabled {
            let _ = disable_raw_mode();
            self.raw_enabled = false;
        }
        let _ = self.terminal.show_cursor();
    }
}

fn terminal_error(operation: &'static str, source: io::Error) -> PersistError {
    PersistError::Io { operation, source }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::fs::File;
    use std::io::{self, Read};
    use std::os::fd::FromRawFd;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::process::{Command, Stdio};

    use super::TerminalGuard;

    const HELPER_ENV: &str = "PERSIST_TERMINAL_PANIC_HELPER";

    #[test]
    fn panic_restore_helper() {
        if std::env::var_os(HELPER_ENV).is_none() {
            return;
        }

        let before = stty_state();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _terminal = TerminalGuard::enter().expect("enter dashboard terminal");
            panic!("injected dashboard panic");
        }));
        assert!(result.is_err());
        assert_eq!(before, stty_state());
    }

    #[test]
    fn panic_restores_terminal_state_in_pty() {
        let (mut master, slave) = open_pty();
        let executable = std::env::current_exe().expect("locate test executable");
        let stdin = slave.try_clone().expect("clone PTY slave for stdin");
        let stdout = slave.try_clone().expect("clone PTY slave for stdout");
        let status = Command::new(executable)
            .args([
                "--exact",
                "dashboard::terminal::tests::panic_restore_helper",
                "--nocapture",
            ])
            .env(HELPER_ENV, "1")
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(slave))
            .status()
            .expect("run PTY panic helper");
        let mut output = String::new();
        let _ = master.read_to_string(&mut output);

        assert!(
            status.success(),
            "PTY panic helper failed: {status}: {output}"
        );
    }

    fn stty_state() -> String {
        let output = Command::new("stty")
            .arg("-g")
            .stdin(Stdio::inherit())
            .output()
            .expect("run stty");
        assert!(
            output.status.success(),
            "stty failed: {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("stty output is UTF-8")
            .trim()
            .to_owned()
    }

    fn open_pty() -> (File, File) {
        let mut master = -1;
        let mut slave = -1;
        let result = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert_eq!(result, 0, "openpty failed: {}", io::Error::last_os_error());
        assert!(master >= 0 && slave >= 0);
        unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) }
    }
}
