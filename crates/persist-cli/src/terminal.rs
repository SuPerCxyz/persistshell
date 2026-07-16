use std::io;

use persist_core::PersistError;

pub struct NonblockingMode {
    fd: libc::c_int,
    original_flags: libc::c_int,
}

impl NonblockingMode {
    pub fn enter(fd: libc::c_int) -> Result<Self, PersistError> {
        let original_flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
        if original_flags < 0 {
            return Err(io_error("fcntl F_GETFL"));
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFL, original_flags | libc::O_NONBLOCK) } < 0 {
            return Err(io_error("fcntl F_SETFL O_NONBLOCK"));
        }
        Ok(Self { fd, original_flags })
    }
}

impl Drop for NonblockingMode {
    fn drop(&mut self) {
        unsafe {
            libc::fcntl(self.fd, libc::F_SETFL, self.original_flags);
        }
    }
}

pub struct RawMode {
    orig: libc::termios,
}

impl RawMode {
    pub fn enter() -> Result<Self, PersistError> {
        let fd = libc::STDIN_FILENO;
        let mut orig = std::mem::MaybeUninit::<libc::termios>::uninit();
        let ret = unsafe { libc::tcgetattr(fd, orig.as_mut_ptr()) };
        if ret < 0 {
            return Err(PersistError::Io {
                operation: "tcgetattr",
                source: io::Error::last_os_error(),
            });
        }
        let orig = unsafe { orig.assume_init() };
        let mut raw = orig;

        raw.c_iflag &= !(libc::IGNBRK
            | libc::BRKINT
            | libc::PARMRK
            | libc::ISTRIP
            | libc::INLCR
            | libc::IGNCR
            | libc::ICRNL
            | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON | libc::ISIG | libc::IEXTEN);
        raw.c_cflag &= !(libc::CSIZE | libc::PARENB);
        raw.c_cflag |= libc::CS8;
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;

        let ret = unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) };
        if ret < 0 {
            return Err(PersistError::Io {
                operation: "tcsetattr",
                source: io::Error::last_os_error(),
            });
        }

        Ok(RawMode { orig })
    }

    pub fn restore(&self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &self.orig);
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        self.restore();
    }
}

fn io_error(operation: &'static str) -> PersistError {
    PersistError::Io {
        operation,
        source: io::Error::last_os_error(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonblocking_mode_restores_original_flags() {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let original = unsafe { libc::fcntl(fds[0], libc::F_GETFL, 0) };
        {
            let _guard = NonblockingMode::enter(fds[0]).unwrap();
            let active = unsafe { libc::fcntl(fds[0], libc::F_GETFL, 0) };
            assert_ne!(active & libc::O_NONBLOCK, 0);
        }
        let restored = unsafe { libc::fcntl(fds[0], libc::F_GETFL, 0) };
        assert_eq!(restored, original);
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
    }
}
