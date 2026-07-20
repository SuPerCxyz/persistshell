use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;

use crate::Result;

use super::unix::{
    cstr, file_from_fd, invalid, io_error, mkdir_private, open_directory, open_directory_at,
    random_incarnation, stat_at, stat_fd, validate_fd, validate_stat, PRIVATE_FILE_MODE,
};
use super::{
    decode_and_validate, encode_envelope, identity_from_parts, ShellStateEnvelope,
    ShellStateIdentity, MAX_SHELL_STATE_BYTES,
};

const STATE_DIR_NAME: &str = "session-state";

pub fn create_identity(runtime_dir: &Path, session_id: u32) -> Result<ShellStateIdentity> {
    if session_id == 0 {
        return Err(invalid("shell state session id must be non-zero"));
    }
    let runtime = open_directory(runtime_dir, "open shell state runtime directory")?;
    validate_fd(&runtime, true)?;
    if mkdir_private(runtime.as_raw_fd(), cstr(STATE_DIR_NAME)?.as_c_str())? {
        runtime
            .sync_all()
            .map_err(|source| io_error("sync shell state runtime directory", source))?;
    }
    let state_dir = open_directory_at(
        runtime.as_raw_fd(),
        cstr(STATE_DIR_NAME)?.as_c_str(),
        "open shell state directory",
    )?;
    validate_fd(&state_dir, true)?;

    let incarnation = random_incarnation()?;
    let filename = format!("{session_id}-{}.json", hex(incarnation));
    identity_from_parts(
        session_id,
        incarnation,
        runtime_dir.join(STATE_DIR_NAME).join(filename),
    )
}

fn open_state_directory(identity: &ShellStateIdentity) -> Result<File> {
    let parent = identity
        .path()
        .parent()
        .ok_or_else(|| invalid("shell state path has no parent"))?;
    let directory = open_directory(parent, "open shell state directory")?;
    validate_fd(&directory, true)?;
    Ok(directory)
}

pub fn write_atomic(identity: &ShellStateIdentity, state: &ShellStateEnvelope) -> Result<()> {
    let encoded = encode_envelope(state)?;
    decode_and_validate(identity, 0, &encoded)?;
    let directory = open_state_directory(identity)?;
    let target = state_filename(identity)?;
    validate_existing_target(directory.as_raw_fd(), &target)?;

    let temp_name = temporary_name(identity, state.sequence)?;
    let temp_fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            temp_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            PRIVATE_FILE_MODE,
        )
    };
    let mut temp = file_from_fd(temp_fd, "create shell state temporary file")?;
    let result = (|| {
        validate_fd(&temp, false)?;
        temp.write_all(&encoded)
            .map_err(|source| io_error("write shell state temporary file", source))?;
        temp.sync_all()
            .map_err(|source| io_error("sync shell state temporary file", source))?;
        if unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                temp_name.as_ptr(),
                directory.as_raw_fd(),
                target.as_ptr(),
            )
        } != 0
        {
            return Err(io_error(
                "replace shell state file",
                std::io::Error::last_os_error(),
            ));
        }
        directory
            .sync_all()
            .map_err(|source| io_error("sync shell state directory", source))
    })();
    if result.is_err() {
        unsafe {
            libc::unlinkat(directory.as_raw_fd(), temp_name.as_ptr(), 0);
        }
    }
    result
}

pub fn read_validated(
    identity: &ShellStateIdentity,
    minimum_sequence: u64,
) -> Result<Option<ShellStateEnvelope>> {
    let directory = open_state_directory(identity)?;
    let target = state_filename(identity)?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            target.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let source = std::io::Error::last_os_error();
        if source.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(io_error("open shell state file", source));
    }
    let mut file = file_from_fd(fd, "open shell state file")?;
    let before = stat_fd(&file)?;
    validate_stat(&before, false)?;
    if before.st_size < 0 || before.st_size as usize > MAX_SHELL_STATE_BYTES {
        return Err(invalid("shell state file exceeds size limit"));
    }
    let mut encoded = Vec::with_capacity(before.st_size as usize);
    Read::by_ref(&mut file)
        .take((MAX_SHELL_STATE_BYTES + 1) as u64)
        .read_to_end(&mut encoded)
        .map_err(|source| io_error("read shell state file", source))?;
    if encoded.len() > MAX_SHELL_STATE_BYTES {
        return Err(invalid("shell state file exceeds size limit"));
    }
    let after = stat_fd(&file)?;
    validate_stat(&after, false)?;
    if before.st_dev != after.st_dev || before.st_ino != after.st_ino {
        return Err(invalid("shell state file changed while reading"));
    }
    decode_and_validate(identity, minimum_sequence, &encoded).map(Some)
}

pub fn remove_validated(identity: &ShellStateIdentity) -> Result<()> {
    let directory = open_state_directory(identity)?;
    let target = state_filename(identity)?;
    let Some(metadata) = stat_at(directory.as_raw_fd(), &target)? else {
        return Ok(());
    };
    validate_stat(&metadata, false)?;
    if unsafe { libc::unlinkat(directory.as_raw_fd(), target.as_ptr(), 0) } != 0 {
        return Err(io_error(
            "remove shell state file",
            std::io::Error::last_os_error(),
        ));
    }
    directory
        .sync_all()
        .map_err(|source| io_error("sync shell state directory", source))
}

fn validate_existing_target(dir_fd: RawFd, target: &CStr) -> Result<()> {
    if let Some(metadata) = stat_at(dir_fd, target)? {
        validate_stat(&metadata, false)?;
    }
    Ok(())
}

fn state_filename(identity: &ShellStateIdentity) -> Result<CString> {
    let name = identity
        .path()
        .file_name()
        .ok_or_else(|| invalid("shell state path has no filename"))?;
    cstr(name)
}

fn temporary_name(identity: &ShellStateIdentity, sequence: u64) -> Result<CString> {
    let nonce = random_incarnation()?;
    cstr(format!(
        ".{}-{sequence}-{}.tmp",
        identity.incarnation_hex(),
        hex(nonce)
    ))
}

fn hex(value: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(32);
    for byte in value {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
