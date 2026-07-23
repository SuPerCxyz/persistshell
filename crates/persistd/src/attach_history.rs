use std::ffi::CString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use persist_core::{PersistError, Result};

const LOG_MODE: u32 = 0o600;
const LOG_DIR_MODE: u32 = 0o700;
const MAX_ROTATED_FILES: u32 = 1024;

pub(crate) fn read_rotated_tail(
    logs_dir: &Path,
    session_id: u32,
    max_files: u32,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    read_rotated_tail_for_uid(logs_dir, session_id, max_files, max_bytes, unsafe {
        libc::getuid()
    })
}

fn read_rotated_tail_for_uid(
    logs_dir: &Path,
    session_id: u32,
    max_files: u32,
    max_bytes: usize,
    expected_uid: u32,
) -> Result<Vec<u8>> {
    if max_bytes == 0 {
        return Ok(Vec::new());
    }
    if max_files > MAX_ROTATED_FILES {
        return Err(PersistError::invalid_argument(
            "session log rotation count exceeds replay limit",
        ));
    }
    let Some(directory) = open_log_directory(logs_dir)? else {
        return Ok(Vec::new());
    };
    validate_directory(&directory, expected_uid)?;

    let mut segments = Vec::new();
    let mut remaining = max_bytes;
    for rotation in 0..=max_files {
        let name = if rotation == 0 {
            format!("{session_id}.log")
        } else {
            format!("{session_id}.log.{rotation}")
        };
        if let Some(mut file) = open_log_at(&directory, &name)? {
            validate_log(&file, expected_uid)?;
            let bytes = read_tail(&mut file, remaining)?;
            remaining -= bytes.len();
            segments.push(bytes);
            if remaining == 0 {
                break;
            }
        }
    }
    segments.reverse();
    Ok(segments.into_iter().flatten().collect())
}

fn open_log_directory(path: &Path) -> Result<Option<File>> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| PersistError::invalid_argument("session log directory contains NUL"))?;
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        return Ok(Some(unsafe { File::from_raw_fd(fd) }));
    }
    let source = std::io::Error::last_os_error();
    if source.kind() == std::io::ErrorKind::NotFound {
        return Ok(None);
    }
    Err(io_error("open session log directory", source))
}

fn open_log_at(directory: &File, name: &str) -> Result<Option<File>> {
    let name = CString::new(name)
        .map_err(|_| PersistError::invalid_argument("session log name contains NUL"))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        return Ok(Some(unsafe { File::from_raw_fd(fd) }));
    }
    let source = std::io::Error::last_os_error();
    if source.kind() == std::io::ErrorKind::NotFound {
        return Ok(None);
    }
    Err(io_error("open session replay log", source))
}

fn validate_directory(directory: &File, expected_uid: u32) -> Result<()> {
    let metadata = directory
        .metadata()
        .map_err(|source| io_error("inspect session log directory", source))?;
    if !metadata.is_dir()
        || metadata.uid() != expected_uid
        || metadata.mode() & 0o777 != LOG_DIR_MODE
    {
        return Err(PersistError::invalid_argument(
            "unsafe session log directory",
        ));
    }
    Ok(())
}

fn validate_log(file: &File, expected_uid: u32) -> Result<()> {
    let metadata = file
        .metadata()
        .map_err(|source| io_error("inspect session replay log", source))?;
    if !metadata.is_file() || metadata.uid() != expected_uid || metadata.mode() & 0o777 != LOG_MODE
    {
        return Err(PersistError::invalid_argument("unsafe session replay log"));
    }
    Ok(())
}

fn read_tail(file: &mut File, max_bytes: usize) -> Result<Vec<u8>> {
    let length = file
        .metadata()
        .map_err(|source| io_error("inspect session replay log length", source))?
        .len();
    let count = usize::try_from(length.min(max_bytes as u64))
        .map_err(|_| PersistError::invalid_argument("session replay length exceeds usize"))?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let offset = i64::try_from(count)
        .map_err(|_| PersistError::invalid_argument("session replay length exceeds i64"))?;
    file.seek(SeekFrom::End(-offset))
        .map_err(|source| io_error("seek session replay log", source))?;
    let mut bytes = vec![0u8; count];
    file.read_exact(&mut bytes)
        .map_err(|source| io_error("read session replay log", source))?;
    Ok(bytes)
}

fn io_error(operation: &'static str, source: std::io::Error) -> PersistError {
    PersistError::Io { operation, source }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{symlink, PermissionsExt};

    use super::*;

    fn private_tempdir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    fn write_private(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn reads_rotated_files_in_chronological_order() {
        let dir = private_tempdir();
        write_private(&dir.path().join("7.log.2"), b"old-");
        write_private(&dir.path().join("7.log.1"), b"middle-");
        write_private(&dir.path().join("7.log"), b"new");

        let output = read_rotated_tail(dir.path(), 7, 2, 64).unwrap();
        assert_eq!(b"old-middle-new", output.as_slice());
    }

    #[test]
    fn truncates_from_the_oldest_bytes_across_files() {
        let dir = private_tempdir();
        write_private(&dir.path().join("4.log.1"), b"abc");
        write_private(&dir.path().join("4.log"), b"def");

        let output = read_rotated_tail(dir.path(), 4, 1, 5).unwrap();
        assert_eq!(b"bcdef", output.as_slice());
        let exact = read_rotated_tail(dir.path(), 4, 1, 6).unwrap();
        assert_eq!(b"abcdef", exact.as_slice());
    }

    #[test]
    fn zero_limit_and_missing_logs_are_empty() {
        let dir = private_tempdir();
        assert!(read_rotated_tail(dir.path(), 1, 3, 0).unwrap().is_empty());
        assert!(read_rotated_tail(dir.path(), 1, 3, 32).unwrap().is_empty());
    }

    #[test]
    fn preserves_non_utf8_bytes() {
        let dir = private_tempdir();
        write_private(&dir.path().join("2.log"), &[0xff, 0x1b, b'[', b'm']);
        assert_eq!(
            vec![0xff, 0x1b, b'[', b'm'],
            read_rotated_tail(dir.path(), 2, 0, 16).unwrap()
        );
    }

    #[test]
    fn rejects_symlink_directory_and_broad_mode() {
        let dir = private_tempdir();
        let target = dir.path().join("target");
        write_private(&target, b"secret");
        symlink(&target, dir.path().join("3.log")).unwrap();
        assert!(read_rotated_tail(dir.path(), 3, 0, 16).is_err());

        fs::remove_file(dir.path().join("3.log")).unwrap();
        fs::create_dir(dir.path().join("3.log")).unwrap();
        assert!(read_rotated_tail(dir.path(), 3, 0, 16).is_err());

        fs::remove_dir(dir.path().join("3.log")).unwrap();
        write_private(&dir.path().join("3.log"), b"broad");
        fs::set_permissions(dir.path().join("3.log"), fs::Permissions::from_mode(0o640)).unwrap();
        assert!(read_rotated_tail(dir.path(), 3, 0, 16).is_err());
    }

    #[test]
    fn rejects_unexpected_owner_without_reading_content() {
        let dir = private_tempdir();
        write_private(&dir.path().join("8.log"), b"owner");
        let wrong_uid = unsafe { libc::getuid() }.wrapping_add(1);
        assert!(
            read_rotated_tail_for_uid(dir.path(), 8, 0, 16, wrong_uid).is_err(),
            "fd metadata owner must be validated"
        );
    }

    #[test]
    fn rejects_symlink_log_directory() {
        let parent = private_tempdir();
        let real_logs = parent.path().join("real");
        fs::create_dir(&real_logs).unwrap();
        fs::set_permissions(&real_logs, fs::Permissions::from_mode(0o700)).unwrap();
        write_private(&real_logs.join("9.log"), b"history");
        let linked_logs = parent.path().join("linked");
        symlink(&real_logs, &linked_logs).unwrap();

        assert!(read_rotated_tail(&linked_logs, 9, 0, 16).is_err());
    }

    #[test]
    fn rejects_excessive_rotation_count() {
        let dir = private_tempdir();
        assert!(read_rotated_tail(dir.path(), 1, MAX_ROTATED_FILES + 1, 16).is_err());
    }

    #[test]
    fn rejects_fifo_without_blocking() {
        let dir = private_tempdir();
        let fifo = dir.path().join("10.log");
        let fifo_c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);

        assert!(read_rotated_tail(dir.path(), 10, 0, 16).is_err());
    }

    #[test]
    fn bounds_large_replay_across_rotations() {
        const REPLAY_BYTES: usize = 512 * 1024;
        let dir = private_tempdir();
        write_private(&dir.path().join("11.log.2"), &vec![b'a'; 300 * 1024]);
        write_private(&dir.path().join("11.log.1"), &vec![b'b'; 300 * 1024]);
        write_private(&dir.path().join("11.log"), &vec![b'c'; 300 * 1024]);

        let output = read_rotated_tail(dir.path(), 11, 2, REPLAY_BYTES).unwrap();
        assert_eq!(output.len(), REPLAY_BYTES);
        assert_eq!(output.first(), Some(&b'b'));
        assert_eq!(output.last(), Some(&b'c'));
    }
}
