use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

use tempfile::{tempdir, TempDir};

use super::*;

fn state(identity: &ShellStateIdentity, sequence: u64, cwd: &str) -> ShellStateEnvelope {
    ShellStateEnvelope::new(
        identity.session_id(),
        identity.incarnation(),
        sequence,
        cwd.into(),
    )
    .expect("state")
}

fn private_root() -> TempDir {
    let root = tempdir().expect("tempdir");
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).expect("private root");
    root
}

#[test]
fn create_identity_prepares_private_state_directory() {
    let root = private_root();
    let identity = create_identity(root.path(), 7).expect("identity");
    let state_dir = root.path().join("session-state");
    let metadata = fs::metadata(&state_dir).expect("state dir");

    assert!(metadata.is_dir());
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    assert_eq!(identity.path().parent(), Some(state_dir.as_path()));
    assert_ne!(identity.incarnation(), [0; 16]);
    assert!(!identity.path().exists());
}

#[test]
fn create_identity_rejects_broad_runtime_directory() {
    let root = tempdir().expect("tempdir");
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).expect("broad root");
    assert!(create_identity(root.path(), 7).is_err());
}

#[test]
fn atomic_write_replaces_complete_state_and_reads_latest() {
    let root = private_root();
    let identity = create_identity(root.path(), 9).expect("identity");

    write_atomic(&identity, &state(&identity, 1, "/first")).expect("first write");
    write_atomic(&identity, &state(&identity, 2, "/second")).expect("second write");

    let loaded = read_validated(&identity, 1)
        .expect("read")
        .expect("state exists");
    assert_eq!(loaded.sequence, 2);
    assert_eq!(loaded.cwd, "/second");
    let metadata = fs::metadata(identity.path()).expect("state metadata");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::read_dir(identity.path().parent().expect("parent"))
            .expect("read state dir")
            .count(),
        1
    );
}

#[test]
fn invalid_replacement_keeps_previous_state() {
    let root = private_root();
    let identity = create_identity(root.path(), 10).expect("identity");
    write_atomic(&identity, &state(&identity, 1, "/kept")).expect("initial write");
    let wrong = ShellStateEnvelope::new(11, [0x22; 16], 2, "/wrong".into()).expect("wrong");

    assert!(write_atomic(&identity, &wrong).is_err());
    assert_eq!(
        read_validated(&identity, 0)
            .expect("read")
            .expect("state")
            .cwd,
        "/kept"
    );
}

#[test]
fn state_io_rejects_target_and_parent_symlinks() {
    let root = private_root();
    let identity = create_identity(root.path(), 11).expect("identity");
    let outside = root.path().join("outside");
    fs::write(&outside, b"unchanged").expect("outside");
    symlink(&outside, identity.path()).expect("target symlink");
    assert!(write_atomic(&identity, &state(&identity, 1, "/safe")).is_err());
    assert_eq!(fs::read(&outside).expect("outside read"), b"unchanged");

    fs::remove_file(identity.path()).expect("remove target symlink");
    let state_dir = root.path().join("session-state");
    let real_dir = root.path().join("real-state");
    fs::rename(&state_dir, &real_dir).expect("move state dir");
    symlink(&real_dir, &state_dir).expect("parent symlink");
    assert!(write_atomic(&identity, &state(&identity, 1, "/safe")).is_err());
}

#[test]
fn read_rejects_bad_mode_non_file_and_oversized_content() {
    let root = private_root();
    let identity = create_identity(root.path(), 12).expect("identity");
    write_atomic(&identity, &state(&identity, 1, "/safe")).expect("write");
    fs::set_permissions(identity.path(), fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(read_validated(&identity, 0).is_err());

    fs::remove_file(identity.path()).expect("remove state");
    fs::create_dir(identity.path()).expect("directory target");
    assert!(read_validated(&identity, 0).is_err());
    fs::remove_dir(identity.path()).expect("remove directory");

    fs::write(identity.path(), vec![b'x'; MAX_SHELL_STATE_BYTES + 1]).expect("oversized");
    fs::set_permissions(identity.path(), fs::Permissions::from_mode(0o600)).expect("chmod");
    assert!(read_validated(&identity, 0).is_err());
}

#[test]
fn validated_remove_never_follows_symlink() {
    let root = private_root();
    let identity = create_identity(root.path(), 13).expect("identity");
    assert!(read_validated(&identity, 0)
        .expect("missing read")
        .is_none());
    write_atomic(&identity, &state(&identity, 1, "/safe")).expect("write");
    remove_validated(&identity).expect("remove");
    assert!(!identity.path().exists());

    let outside = root.path().join("outside");
    fs::write(&outside, b"keep").expect("outside");
    symlink(&outside, identity.path()).expect("symlink");
    assert!(remove_validated(&identity).is_err());
    assert_eq!(fs::read(&outside).expect("outside read"), b"keep");
}

#[test]
fn private_attribute_validation_rejects_owner_mode_and_kind() {
    let uid = unsafe { libc::geteuid() };
    assert!(validate_private_attributes(uid, uid, libc::S_IFREG | 0o600, false).is_ok());
    assert!(validate_private_attributes(uid + 1, uid, libc::S_IFREG | 0o600, false).is_err());
    assert!(validate_private_attributes(uid, uid, libc::S_IFREG | 0o640, false).is_err());
    assert!(
        validate_private_attributes(uid, uid, libc::S_IFREG | libc::S_ISUID | 0o600, false)
            .is_err()
    );
    assert!(validate_private_attributes(uid, uid, libc::S_IFDIR | 0o700, false).is_err());
    assert!(validate_private_attributes(uid, uid, libc::S_IFDIR | 0o700, true).is_ok());
}

#[test]
#[ignore = "manual shell state commit benchmark"]
fn shell_state_commit_benchmark() {
    use std::time::Instant;

    let root = private_root();
    let identity = create_identity(root.path(), 54).expect("identity");
    let started = Instant::now();
    let mut max_us = 0_u128;
    for sequence in 1..=1_000 {
        let sample_started = Instant::now();
        write_atomic(
            &identity,
            &ShellStateEnvelope::new(54, identity.incarnation(), sequence, "/srv/final".into())
                .expect("state"),
        )
        .expect("write");
        max_us = max_us.max(sample_started.elapsed().as_micros());
    }
    let total_us = started.elapsed().as_micros();
    assert_eq!(
        read_validated(&identity, 1_000)
            .expect("read")
            .expect("state")
            .sequence,
        1_000
    );
    eprintln!(
        "commits=1000,total_us={total_us},mean_us={},max_us={max_us}",
        total_us / 1_000
    );
}
