use std::io;
use std::os::fd::RawFd;

use persist_core::{PersistError, Result};

const CONTEXT_SAMPLE_NANOS: libc::c_long = 100_000_000;

pub(super) struct ContextTimer {
    fd: RawFd,
}

impl ContextTimer {
    pub(super) fn start() -> Result<Self> {
        let fd = unsafe { libc::timerfd_create(libc::CLOCK_MONOTONIC, libc::TFD_CLOEXEC) };
        if fd < 0 {
            return Err(io_error(
                "create attach context timer",
                io::Error::last_os_error(),
            ));
        }
        let interval = libc::timespec {
            tv_sec: 0,
            tv_nsec: CONTEXT_SAMPLE_NANOS,
        };
        let spec = libc::itimerspec {
            it_interval: interval,
            it_value: interval,
        };
        if unsafe { libc::timerfd_settime(fd, 0, &spec, std::ptr::null_mut()) } != 0 {
            let source = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(io_error("arm attach context timer", source));
        }
        Ok(Self { fd })
    }

    pub(super) fn fd(&self) -> RawFd {
        self.fd
    }

    pub(super) fn consume(&self) -> Result<()> {
        let mut expirations = 0u64;
        let count = unsafe {
            libc::read(
                self.fd,
                (&mut expirations as *mut u64).cast(),
                std::mem::size_of::<u64>(),
            )
        };
        if count as usize == std::mem::size_of::<u64>() {
            return Ok(());
        }
        Err(io_error(
            "read attach context timer",
            io::Error::last_os_error(),
        ))
    }
}

impl Drop for ContextTimer {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

fn io_error(operation: &'static str, source: io::Error) -> PersistError {
    PersistError::Io { operation, source }
}
