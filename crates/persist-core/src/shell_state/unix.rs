use std::ffi::{CStr, CString, OsStr};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::{PersistError, Result};

pub(super) const PRIVATE_DIR_MODE: libc::mode_t = 0o700;
pub(super) const PRIVATE_FILE_MODE: libc::mode_t = 0o600;

pub(super) fn mkdir_private(dir_fd: RawFd, name: &CStr) -> Result<bool> {
    if unsafe { libc::mkdirat(dir_fd, name.as_ptr(), PRIVATE_DIR_MODE) } == 0 {
        return Ok(true);
    }
    let source = std::io::Error::last_os_error();
    if source.kind() == std::io::ErrorKind::AlreadyExists {
        Ok(false)
    } else {
        Err(io_error("create shell state directory", source))
    }
}

pub(super) fn open_directory(path: &Path, operation: &'static str) -> Result<File> {
    let path = cstr(path.as_os_str())?;
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    file_from_fd(fd, operation)
}

pub(super) fn open_directory_at(
    dir_fd: RawFd,
    name: &CStr,
    operation: &'static str,
) -> Result<File> {
    let fd = unsafe {
        libc::openat(
            dir_fd,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    file_from_fd(fd, operation)
}

pub(super) fn file_from_fd(fd: RawFd, operation: &'static str) -> Result<File> {
    if fd < 0 {
        return Err(io_error(operation, std::io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub(super) fn stat_at(dir_fd: RawFd, name: &CStr) -> Result<Option<libc::stat>> {
    let mut metadata = MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            dir_fd,
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(Some(unsafe { metadata.assume_init() }));
    }
    let source = std::io::Error::last_os_error();
    if source.kind() == std::io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err(io_error("inspect shell state file", source))
    }
}

pub(super) fn stat_fd(file: &File) -> Result<libc::stat> {
    let mut metadata = MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(file.as_raw_fd(), metadata.as_mut_ptr()) } != 0 {
        return Err(io_error(
            "inspect shell state descriptor",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(unsafe { metadata.assume_init() })
}

pub(super) fn validate_fd(file: &File, directory: bool) -> Result<()> {
    validate_stat(&stat_fd(file)?, directory)
}

pub(super) fn validate_stat(metadata: &libc::stat, directory: bool) -> Result<()> {
    validate_private_attributes(
        metadata.st_uid,
        unsafe { libc::geteuid() },
        metadata.st_mode,
        directory,
    )
}

pub(super) fn validate_private_attributes(
    actual_uid: libc::uid_t,
    expected_uid: libc::uid_t,
    mode: libc::mode_t,
    directory: bool,
) -> Result<()> {
    let expected_kind = if directory {
        libc::S_IFDIR
    } else {
        libc::S_IFREG
    };
    let expected_mode = if directory {
        PRIVATE_DIR_MODE
    } else {
        PRIVATE_FILE_MODE
    };
    if actual_uid != expected_uid
        || mode & libc::S_IFMT != expected_kind
        || mode & 0o7777 != expected_mode
    {
        return Err(invalid("shell state owner, type, or mode is invalid"));
    }
    Ok(())
}

pub(super) fn random_incarnation() -> Result<[u8; 16]> {
    let mut random = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open("/dev/urandom")
        .map_err(|source| io_error("open urandom for shell state", source))?;
    let mut value = [0u8; 16];
    random
        .read_exact(&mut value)
        .map_err(|source| io_error("read shell state incarnation", source))?;
    if value == [0; 16] {
        return Err(invalid("shell state incarnation must be non-zero"));
    }
    Ok(value)
}

pub(super) fn cstr(value: impl AsRef<OsStr>) -> Result<CString> {
    CString::new(value.as_ref().as_bytes()).map_err(|_| invalid("shell state path contains NUL"))
}

pub(super) fn invalid(message: impl Into<String>) -> PersistError {
    PersistError::invalid_argument(message)
}

pub(super) fn io_error(operation: &'static str, source: std::io::Error) -> PersistError {
    PersistError::Io { operation, source }
}
