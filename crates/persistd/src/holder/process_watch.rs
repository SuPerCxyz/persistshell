use std::fs;
use std::os::fd::RawFd;
use std::time::{Duration, Instant};

use persist_core::{PersistError, Result};

const EXIT_TIMEOUT: Duration = Duration::from_secs(3);
pub(super) const FALLBACK_TICK: Duration = Duration::from_millis(25);

pub(super) struct ProcessExit {
    watcher: ProcessWatcher,
}

impl ProcessExit {
    pub(super) fn watch(pid: u32) -> Result<Self> {
        match PidFd::open(pid) {
            Ok(pid_fd) => Ok(Self {
                watcher: ProcessWatcher::PidFd(pid_fd),
            }),
            Err(source) if pidfd_fallback_allowed(&source) => Self::watch_procfs(pid),
            Err(source) => Err(io_error("open persist-holder pidfd", source)),
        }
    }

    pub(super) fn has_exited(&self) -> Result<bool> {
        match &self.watcher {
            ProcessWatcher::PidFd(pid_fd) => pid_fd.has_exited(),
            ProcessWatcher::Procfs(identity) => identity.has_exited(),
        }
    }

    pub(super) fn wait(&self) -> Result<()> {
        self.wait_for(EXIT_TIMEOUT)
    }

    fn wait_for(&self, timeout: Duration) -> Result<()> {
        match &self.watcher {
            ProcessWatcher::PidFd(pid_fd) => pid_fd.wait(timeout),
            ProcessWatcher::Procfs(identity) => identity.wait(timeout),
        }
    }

    pub(super) fn poll_fd(&self) -> Option<RawFd> {
        match &self.watcher {
            ProcessWatcher::PidFd(pid_fd) => Some(pid_fd.fd),
            ProcessWatcher::Procfs(_) => None,
        }
    }

    fn watch_procfs(pid: u32) -> Result<Self> {
        Ok(Self {
            watcher: ProcessWatcher::Procfs(ProcessIdentity::capture(pid)?),
        })
    }
}

enum ProcessWatcher {
    PidFd(PidFd),
    Procfs(ProcessIdentity),
}

struct ProcessIdentity {
    pid: u32,
    start_time: u64,
}

impl ProcessIdentity {
    fn capture(pid: u32) -> Result<Self> {
        let stat = read_process_stat(pid)
            .map_err(|source| io_error("read persist-holder process identity", source))?;
        Ok(Self {
            pid,
            start_time: stat.start_time,
        })
    }

    fn has_exited(&self) -> Result<bool> {
        match read_process_stat(self.pid) {
            Ok(stat) => {
                Ok(stat.start_time != self.start_time || matches!(stat.state, b'Z' | b'X' | b'x'))
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(source) => Err(io_error("inspect persist-holder process identity", source)),
        }
    }

    fn wait(&self, timeout: Duration) -> Result<()> {
        let timer = TimerFd::periodic(FALLBACK_TICK)?;
        let deadline = Instant::now() + timeout;
        loop {
            if self.has_exited()? {
                return Ok(());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(holder_wait_timeout());
            }
            timer.wait(remaining)?;
        }
    }
}

struct ProcessStat {
    state: u8,
    start_time: u64,
}

fn read_process_stat(pid: u32) -> std::io::Result<ProcessStat> {
    let contents = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    parse_process_stat(&contents).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid persist-holder process stat",
        )
    })
}

fn parse_process_stat(contents: &str) -> Option<ProcessStat> {
    let close = contents.rfind(')')?;
    let mut fields = contents.get(close + 1..)?.split_whitespace();
    let state = fields.next()?.as_bytes();
    if state.len() != 1 {
        return None;
    }
    let start_time = fields.nth(18)?.parse().ok()?;
    Some(ProcessStat {
        state: state[0],
        start_time,
    })
}

struct PidFd {
    fd: RawFd,
}

impl PidFd {
    fn open(pid: u32) -> std::io::Result<Self> {
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) as RawFd };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { fd })
    }

    fn has_exited(&self) -> Result<bool> {
        let mut pollfd = pollfd(self.fd);
        let ready = unsafe { libc::poll(&mut pollfd, 1, 0) };
        if ready < 0 {
            return Err(io_error(
                "inspect persist-holder process",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(ready > 0)
    }

    fn wait(&self, timeout: Duration) -> Result<()> {
        let mut pollfd = pollfd(self.fd);
        let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_millis(timeout)) };
        if ready > 0 {
            return Ok(());
        }
        wait_error(ready, "wait for persist-holder pidfd")
    }
}

impl Drop for PidFd {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

pub(super) struct TimerFd {
    fd: RawFd,
}

impl TimerFd {
    pub(super) fn one_shot(duration: Duration) -> Result<Self> {
        Self::new(duration, Duration::ZERO)
    }

    fn periodic(duration: Duration) -> Result<Self> {
        Self::new(duration, duration)
    }

    fn new(initial: Duration, interval: Duration) -> Result<Self> {
        let fd = unsafe { libc::timerfd_create(libc::CLOCK_MONOTONIC, libc::TFD_CLOEXEC) };
        if fd < 0 {
            return Err(io_error(
                "create persist-holder fallback timer",
                std::io::Error::last_os_error(),
            ));
        }
        let spec = libc::itimerspec {
            it_interval: duration_timespec(interval),
            it_value: duration_timespec(initial),
        };
        if unsafe { libc::timerfd_settime(fd, 0, &spec, std::ptr::null_mut()) } != 0 {
            let source = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(io_error("arm persist-holder fallback timer", source));
        }
        Ok(Self { fd })
    }

    pub(super) fn fd(&self) -> RawFd {
        self.fd
    }

    fn wait(&self, timeout: Duration) -> Result<()> {
        let mut pollfd = pollfd(self.fd);
        let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_millis(timeout)) };
        if ready <= 0 {
            return wait_error(ready, "wait for persist-holder fallback timer");
        }
        let mut expirations = 0u64;
        let read = unsafe {
            libc::read(
                self.fd,
                (&mut expirations as *mut u64).cast(),
                std::mem::size_of::<u64>(),
            )
        };
        if read < 0 {
            return Err(io_error(
                "read persist-holder fallback timer",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(())
    }
}

impl Drop for TimerFd {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

fn pollfd(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}

fn timeout_millis(duration: Duration) -> i32 {
    duration.as_millis().min(i32::MAX as u128) as i32
}

fn duration_timespec(duration: Duration) -> libc::timespec {
    libc::timespec {
        tv_sec: duration.as_secs().min(libc::time_t::MAX as u64) as libc::time_t,
        tv_nsec: duration.subsec_nanos() as libc::c_long,
    }
}

fn pidfd_fallback_allowed(source: &std::io::Error) -> bool {
    matches!(
        source.raw_os_error(),
        Some(libc::ENOSYS | libc::EINVAL | libc::EPERM)
    )
}

fn wait_error(ready: i32, operation: &'static str) -> Result<()> {
    if ready < 0 {
        return Err(io_error(operation, std::io::Error::last_os_error()));
    }
    Err(holder_wait_timeout())
}

fn holder_wait_timeout() -> PersistError {
    PersistError::internal_error("persist-holder did not exit after ShutdownAll")
}

fn io_error(operation: &'static str, source: std::io::Error) -> PersistError {
    PersistError::Io { operation, source }
}

#[cfg(test)]
mod tests;
