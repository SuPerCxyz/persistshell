use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{PersistError, Result};

use crate::command_history_format::{
    compact, encode_record, estimated_size, load_state, write_state,
};

pub(crate) const MAGIC: &[u8; 8] = b"PSHIST01";
pub(crate) const HEADER_LEN: usize = 24;
pub(crate) const RECORD_META_LEN: usize = 20;
pub const MAX_COMMAND_BYTES: usize = 64 * 1024;
pub const MAX_HISTORY_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_HISTORY_RECORDS: usize = 10_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandRecord {
    pub sequence: u64,
    pub completed_at_ms: u64,
    pub shell: String,
    pub command: Vec<u8>,
}

pub fn command_history_path(data_dir: &Path, session_id: u32) -> PathBuf {
    data_dir
        .join("history")
        .join(format!("{session_id}.commands"))
}

pub fn append_command(path: &Path, shell: &str, command: &[u8]) -> Result<CommandRecord> {
    validate_record(shell, command)?;
    prepare_parent(path)?;
    let mut file = open_private(path)?;
    let _lock = FileLock::exclusive(&file)?;
    let mut state = load_state(&mut file)?;
    let record = CommandRecord {
        sequence: state.next_sequence,
        completed_at_ms: now_millis(),
        shell: shell.to_string(),
        command: command.to_vec(),
    };
    if file
        .metadata()
        .map_err(|source| io_error("inspect command history", source))?
        .len()
        == 0
    {
        state.records.push(record.clone());
        state.next_sequence = record.sequence.saturating_add(1);
        write_state(&mut file, &state)?;
        return Ok(record);
    }
    let encoded_size = 4 + RECORD_META_LEN + record.shell.len() + record.command.len();
    let current_size = estimated_size(&state.records);
    if state.records.len() < MAX_HISTORY_RECORDS
        && current_size.saturating_add(encoded_size) <= MAX_HISTORY_BYTES as usize
    {
        append_record(&mut file, &record, state.records.len())?;
        return Ok(record);
    }
    state.records.push(record.clone());
    state.next_sequence = state.next_sequence.saturating_add(1);
    compact(&mut state.records);
    write_state(&mut file, &state)?;
    Ok(record)
}

fn append_record(file: &mut File, record: &CommandRecord, count: usize) -> Result<()> {
    let mut encoded = Vec::new();
    encode_record(record, &mut encoded)?;
    file.seek(SeekFrom::End(0))
        .and_then(|_| file.write_all(&encoded))
        .map_err(|source| io_error("append command history", source))?;
    file.seek(SeekFrom::Start(8))
        .and_then(|_| file.write_all(&record.sequence.saturating_add(1).to_be_bytes()))
        .and_then(|_| file.write_all(&(count.saturating_add(1) as u64).to_be_bytes()))
        .and_then(|_| file.sync_data())
        .map_err(|source| io_error("update command history header", source))?;
    Ok(())
}

pub fn read_commands_desc(path: &Path, offset: usize, limit: usize) -> Result<Vec<CommandRecord>> {
    if limit == 0 || !path.exists() {
        return Ok(Vec::new());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|source| io_error("open command history", source))?;
    let _lock = FileLock::shared(&file)?;
    let state = load_state(&mut file)?;
    Ok(state
        .records
        .into_iter()
        .rev()
        .skip(offset)
        .take(limit)
        .collect())
}

pub fn command_count(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|source| io_error("open command history", source))?;
    let _lock = FileLock::shared(&file)?;
    Ok(load_state(&mut file)?.records.len())
}

pub(crate) fn validate_record(shell: &str, command: &[u8]) -> Result<()> {
    if shell.is_empty() || shell.len() > u16::MAX as usize {
        return Err(PersistError::invalid_argument("invalid history shell name"));
    }
    if command.is_empty() {
        return Err(PersistError::invalid_argument("empty history command"));
    }
    if command.len() > MAX_COMMAND_BYTES {
        return Err(PersistError::invalid_argument(
            "history command is too large",
        ));
    }
    Ok(())
}

fn prepare_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| PersistError::invalid_argument("history path has no parent"))?;
    fs::create_dir_all(parent).map_err(|source| io_error("create history directory", source))?;
    let metadata = fs::symlink_metadata(parent)
        .map_err(|source| io_error("inspect history directory", source))?;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(PersistError::invalid_argument(
            "history directory is not a private owned directory",
        ));
    }
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .map_err(|source| io_error("set history directory permissions", source))
}

fn open_private(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|source| io_error("open command history", source))?;
    let metadata = file
        .metadata()
        .map_err(|source| io_error("inspect command history", source))?;
    if !metadata.is_file() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(PersistError::invalid_argument(
            "command history is not a private owned file",
        ));
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|source| io_error("set command history permissions", source))?;
    Ok(file)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

pub(crate) fn io_error(operation: &'static str, source: io::Error) -> PersistError {
    PersistError::Io { operation, source }
}

struct FileLock {
    fd: std::os::fd::RawFd,
}

impl FileLock {
    fn exclusive(file: &File) -> Result<Self> {
        Self::acquire(file, libc::LOCK_EX)
    }

    fn shared(file: &File) -> Result<Self> {
        Self::acquire(file, libc::LOCK_SH)
    }

    fn acquire(file: &File, operation: libc::c_int) -> Result<Self> {
        let fd = file.as_raw_fd();
        let result = unsafe { libc::flock(fd, operation) };
        if result != 0 {
            return Err(io_error("lock command history", io::Error::last_os_error()));
        }
        Ok(Self { fd })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.fd, libc::LOCK_UN);
        }
    }
}
