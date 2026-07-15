use std::io;

use persist_core::PersistError;

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
