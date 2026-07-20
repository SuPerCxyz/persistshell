use std::path::{Path, PathBuf};

use persist_core::shell_state::{
    identity_from_parts, read_validated, ShellStateEnvelope, ShellStateIdentity,
};

pub(super) fn validated_identity(
    state_dir: &Path,
    session_id: u32,
    incarnation: [u8; 16],
    state_file: &str,
) -> Option<ShellStateIdentity> {
    let identity = identity_from_parts(session_id, incarnation, PathBuf::from(state_file)).ok()?;
    (identity.path().parent() == Some(state_dir)).then_some(identity)
}

pub(super) fn capture(
    identity: &ShellStateIdentity,
    minimum_sequence: u64,
) -> Option<ShellStateEnvelope> {
    read_validated(identity, minimum_sequence).ok().flatten()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{symlink, PermissionsExt};

    use persist_core::shell_state::{write_atomic, ShellStateEnvelope};

    use super::*;

    const SESSION_ID: u32 = 54;
    const INCARNATION: [u8; 16] = [0x54; 16];

    #[test]
    fn identity_must_belong_to_configured_state_directory() {
        let temp = tempfile::tempdir().unwrap();
        let state_dir = temp.path().join("session-state");
        let expected = state_dir.join(filename());
        assert!(validated_identity(
            &state_dir,
            SESSION_ID,
            INCARNATION,
            expected.to_str().unwrap()
        )
        .is_some());

        let other = temp.path().join("other/session-state").join(filename());
        assert!(
            validated_identity(&state_dir, SESSION_ID, INCARNATION, other.to_str().unwrap())
                .is_none()
        );
    }

    #[test]
    fn capture_degrades_for_missing_corrupt_mismatched_and_symlink_state() {
        let temp = tempfile::tempdir().unwrap();
        let state_dir = create_state_dir(temp.path());
        let identity = identity(&state_dir);

        assert_eq!(capture(&identity, 0), None);
        write_private(identity.path(), b"not-json");
        assert_eq!(capture(&identity, 0), None);

        let mismatched =
            ShellStateEnvelope::new(SESSION_ID, [0x55; 16], 1, "/srv/wrong".into()).unwrap();
        write_private(
            identity.path(),
            &persist_core::shell_state::encode_envelope(&mismatched).unwrap(),
        );
        assert_eq!(capture(&identity, 0), None);

        std::fs::remove_file(identity.path()).unwrap();
        let target = temp.path().join("target");
        write_private(&target, b"not-json");
        symlink(&target, identity.path()).unwrap();
        assert_eq!(capture(&identity, 0), None);
    }

    #[test]
    fn capture_returns_valid_final_state_and_honors_sequence() {
        let temp = tempfile::tempdir().unwrap();
        let state_dir = create_state_dir(temp.path());
        let identity = identity(&state_dir);
        let state =
            ShellStateEnvelope::new(SESSION_ID, INCARNATION, 3, "/srv/final".into()).unwrap();
        write_atomic(&identity, &state).unwrap();

        assert_eq!(capture(&identity, 3), Some(state));
        assert_eq!(capture(&identity, 4), None);
    }

    fn create_state_dir(root: &Path) -> PathBuf {
        let state_dir = root.join("session-state");
        std::fs::create_dir(&state_dir).unwrap();
        std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        state_dir
    }

    fn identity(state_dir: &Path) -> ShellStateIdentity {
        validated_identity(
            state_dir,
            SESSION_ID,
            INCARNATION,
            state_dir.join(filename()).to_str().unwrap(),
        )
        .unwrap()
    }

    fn filename() -> String {
        format!("{SESSION_ID}-{}.json", "54545454545454545454545454545454")
    }

    fn write_private(path: &Path, contents: &[u8]) {
        std::fs::write(path, contents).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}
