use std::fs;
use std::ops::Deref;
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::command_history::{
    append_command, command_count, command_history_path, read_commands_desc, MAX_COMMAND_BYTES,
    MAX_HISTORY_BYTES,
};

#[test]
fn append_and_read_newest_first() {
    let dir = tempfile_dir("order");
    let path = command_history_path(&dir, 7);
    append_command(&path, "bash", b"echo first").expect("append first");
    append_command(&path, "bash", b"echo second").expect("append second");

    let records = read_commands_desc(&path, 0, 50).expect("read records");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].command, b"echo second");
    assert_eq!(records[1].command, b"echo first");
    assert!(records[0].sequence > records[1].sequence);
}

#[test]
fn pagination_and_multiline_are_preserved() {
    let dir = tempfile_dir("pages");
    let path = command_history_path(&dir, 2);
    append_command(&path, "zsh", b"printf '%s\n' \\\none two").expect("append multiline");
    append_command(&path, "zsh", b"pwd").expect("append pwd");

    let first = read_commands_desc(&path, 0, 1).expect("first page");
    let second = read_commands_desc(&path, 1, 1).expect("second page");
    assert_eq!(first[0].command, b"pwd");
    assert!(second[0].command.contains(&b'\n'));
}

#[test]
fn private_permissions_are_enforced() {
    let dir = tempfile_dir("permissions");
    let path = command_history_path(&dir, 3);
    append_command(&path, "fish", b"echo safe").expect("append");

    assert_eq!(
        fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn empty_and_oversized_commands_are_rejected() {
    let dir = tempfile_dir("limits");
    let path = command_history_path(&dir, 4);
    assert!(append_command(&path, "bash", b"").is_err());
    assert!(append_command(&path, "bash", &vec![b'x'; MAX_COMMAND_BYTES + 1]).is_err());
}

#[test]
fn corrupted_history_is_rejected() {
    let dir = tempfile_dir("corrupt");
    let path = command_history_path(&dir, 5);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"not-history").unwrap();
    assert!(read_commands_desc(&path, 0, 50).is_err());
}

#[test]
fn command_count_tracks_records() {
    let dir = tempfile_dir("count");
    let path = command_history_path(&dir, 6);
    assert_eq!(command_count(&path).unwrap(), 0);
    append_command(&path, "bash", b"true").unwrap();
    assert_eq!(command_count(&path).unwrap(), 1);
}

#[test]
fn symlink_history_is_rejected_without_touching_target() {
    let dir = tempfile_dir("symlink");
    let path = command_history_path(&dir, 8);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let target = dir.join("protected");
    fs::write(&target, b"keep-me").unwrap();
    symlink(&target, &path).unwrap();

    assert!(append_command(&path, "bash", b"echo unsafe").is_err());
    assert_eq!(fs::read(target).unwrap(), b"keep-me");
}

#[test]
fn history_is_compacted_below_size_limit() {
    let dir = tempfile_dir("compact");
    let path = command_history_path(&dir, 9);
    let command = vec![b'x'; 60 * 1024];
    for _ in 0..80 {
        append_command(&path, "bash", &command).unwrap();
    }

    assert!(fs::metadata(&path).unwrap().len() <= MAX_HISTORY_BYTES);
    assert!(command_count(&path).unwrap() < 80);
}

#[test]
fn concurrent_appends_keep_every_record() {
    let dir = tempfile_dir("concurrent");
    let path = command_history_path(&dir, 10);
    let workers = (0..4)
        .map(|worker| {
            let path = path.clone();
            std::thread::spawn(move || {
                for number in 0..20 {
                    let command = format!("echo {worker}-{number}");
                    append_command(&path, "fish", command.as_bytes()).unwrap();
                }
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().unwrap();
    }

    assert_eq!(command_count(&path).unwrap(), 80);
}

#[test]
fn append_recovers_one_record_written_before_header_update() {
    let dir = tempfile_dir("recover-header");
    let path = command_history_path(&dir, 11);
    append_command(&path, "bash", b"echo first").unwrap();
    append_command(&path, "bash", b"echo second").unwrap();
    let mut bytes = fs::read(&path).unwrap();
    bytes[8..16].copy_from_slice(&2u64.to_be_bytes());
    bytes[16..24].copy_from_slice(&1u64.to_be_bytes());
    fs::write(&path, bytes).unwrap();

    append_command(&path, "bash", b"echo third").unwrap();
    let records = read_commands_desc(&path, 0, 10).unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].sequence, 3);
    assert_eq!(records[1].sequence, 2);
}

fn tempfile_dir(name: &str) -> TempDir {
    let path = std::env::temp_dir().join(format!(
        "persistshell-command-history-{name}-{}-{}",
        std::process::id(),
        crate::command_history_tests::unique_id()
    ));
    fs::create_dir_all(&path).expect("create temp directory");
    TempDir(path)
}

fn unique_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

struct TempDir(PathBuf);

impl Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
