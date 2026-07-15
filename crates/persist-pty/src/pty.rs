use std::ffi::CStr;
use std::os::unix::io::RawFd;
use std::path::PathBuf;

use persist_core::{PersistError, Result};

pub fn open_pty() -> Result<(RawFd, PathBuf)> {
    let master_fd = open_ptm()?;
    grant_pt(master_fd)?;
    unlock_pt(master_fd)?;
    let slave_path = pts_name(master_fd)?;
    Ok((master_fd, slave_path))
}

fn open_ptm() -> Result<RawFd> {
    let fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if fd < 0 {
        return Err(PersistError::Io {
            operation: "posix_openpt",
            source: std::io::Error::last_os_error(),
        });
    }
    Ok(fd)
}

fn grant_pt(fd: RawFd) -> Result<()> {
    let result = unsafe { libc::grantpt(fd) };
    if result != 0 {
        return Err(PersistError::Io {
            operation: "grantpt",
            source: std::io::Error::last_os_error(),
        });
    }
    Ok(())
}

fn unlock_pt(fd: RawFd) -> Result<()> {
    let result = unsafe { libc::unlockpt(fd) };
    if result != 0 {
        return Err(PersistError::Io {
            operation: "unlockpt",
            source: std::io::Error::last_os_error(),
        });
    }
    Ok(())
}

fn pts_name(master_fd: RawFd) -> Result<PathBuf> {
    let mut buf = [0i8; 4096];
    let result = unsafe { libc::ptsname_r(master_fd, buf.as_mut_ptr(), buf.len()) };
    if result != 0 {
        return Err(PersistError::Io {
            operation: "ptsname_r",
            source: std::io::Error::last_os_error(),
        });
    }
    let c_str = unsafe { CStr::from_ptr(buf.as_ptr()) };
    Ok(PathBuf::from(c_str.to_str().map_err(|_| {
        PersistError::invalid_argument("ptsname returned non-UTF8 path")
    })?))
}

pub fn set_nonblocking(fd: RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(PersistError::Io {
            operation: "fcntl F_GETFL",
            source: std::io::Error::last_os_error(),
        });
    }
    let result = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if result < 0 {
        return Err(PersistError::Io {
            operation: "fcntl F_SETFL O_NONBLOCK",
            source: std::io::Error::last_os_error(),
        });
    }
    Ok(())
}

pub fn detect_shell() -> String {
    // 1. Try getpwuid_r
    if let Some(shell) = shell_from_passwd() {
        if !shell.is_empty() {
            return shell;
        }
    }
    // 2. Try SHELL env var
    if let Ok(shell) = std::env::var("SHELL") {
        if !shell.is_empty() {
            return shell;
        }
    }
    // 3. Fallback
    "/bin/sh".to_string()
}

fn shell_from_passwd() -> Option<String> {
    let uid = unsafe { libc::getuid() };
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0u8; 16384];
    let mut result: *mut libc::passwd = std::ptr::null_mut();

    let ret = unsafe {
        libc::getpwuid_r(
            uid,
            &mut pwd,
            buf.as_mut_ptr() as *mut i8,
            buf.len(),
            &mut result,
        )
    };

    if ret != 0 || result.is_null() || pwd.pw_shell.is_null() {
        return None;
    }

    let shell_cstr = unsafe { CStr::from_ptr(pwd.pw_shell) };
    shell_cstr.to_str().ok().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_shell_returns_valid_path() {
        let shell = detect_shell();
        assert!(!shell.is_empty(), "shell should not be empty");
        assert!(
            std::path::Path::new(&shell).exists(),
            "shell path '{shell}' should exist"
        );
    }

    #[test]
    fn open_pty_creates_valid_pair() {
        let (master, slave_path) = open_pty().expect("open pty");
        assert!(master >= 0, "master fd should be valid");
        assert!(
            slave_path.starts_with("/dev/pts/"),
            "slave path should be /dev/pts/N, got {slave_path:?}"
        );
        let _ = unsafe { libc::close(master) };
    }

    #[test]
    fn set_nonblocking_works() {
        let (master, _slave_path) = open_pty().expect("open pty");
        set_nonblocking(master).expect("set nonblocking");
        let flags = unsafe { libc::fcntl(master, libc::F_GETFL) };
        assert!(flags & libc::O_NONBLOCK != 0, "O_NONBLOCK should be set");
        let _ = unsafe { libc::close(master) };
    }
}
