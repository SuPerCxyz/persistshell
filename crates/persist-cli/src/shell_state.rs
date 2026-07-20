use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::PathBuf;

use persist_core::shell_state::{
    identity_from_parts, read_validated, write_atomic, EnvironmentPolicy, EnvironmentSnapshot,
    ShellStateEnvelope, MAX_SHELL_CWD_BYTES, STATE_ENV_INCLUDE, STATE_ENV_MAX_BYTES,
    STATE_ENV_MAX_VARIABLES, STATE_ENV_POLICY_FINGERPRINT,
};
use persist_core::{PersistError, Result};

const STATE_FILE: &str = "PERSIST_STATE_FILE";
const SESSION_ID: &str = "PERSIST_STATE_SESSION_ID";
const INCARNATION: &str = "PERSIST_STATE_INCARNATION";
const SEQUENCE: &str = "PERSIST_STATE_SEQUENCE";

pub(crate) fn commit_from_reader<I, K, V, R>(environment: I, reader: R) -> Result<()>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<OsString>,
    V: Into<OsString>,
    R: Read,
{
    let environment = environment
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect::<Vec<_>>();
    let session_id = required_control(&environment, SESSION_ID)?
        .parse::<u32>()
        .map_err(|_| invalid("invalid shell state session id"))?;
    let incarnation = parse_incarnation(&required_control(&environment, INCARNATION)?)?;
    let sequence = required_control(&environment, SEQUENCE)?
        .parse::<u64>()
        .map_err(|_| invalid("invalid shell state sequence"))?;
    let identity = identity_from_parts(
        session_id,
        incarnation,
        PathBuf::from(required_control(&environment, STATE_FILE)?),
    )?;

    let cwd = read_cwd(reader)?;
    let state = build_state(
        &environment,
        &identity,
        session_id,
        incarnation,
        sequence,
        cwd,
    )?;
    write_atomic(&identity, &state)
}

fn build_state(
    environment: &[(OsString, OsString)],
    identity: &persist_core::shell_state::ShellStateIdentity,
    session_id: u32,
    incarnation: [u8; 16],
    sequence: u64,
    cwd: String,
) -> Result<ShellStateEnvelope> {
    let previous = read_validated(identity, 0)
        .ok()
        .flatten()
        .and_then(|state| state.environment);
    let Some(policy) = parse_policy(environment) else {
        return match previous
            .as_ref()
            .and_then(|snapshot| snapshot.preserved().ok())
        {
            Some(snapshot) => {
                ShellStateEnvelope::new_v2(session_id, incarnation, sequence, cwd, snapshot)
            }
            None => ShellStateEnvelope::new(session_id, incarnation, sequence, cwd),
        };
    };
    let snapshot = collect_environment(environment, &policy)
        .and_then(|current| EnvironmentSnapshot::capture(&policy, previous.as_ref(), current))
        .or_else(|_| EnvironmentSnapshot::fallback(&policy, previous.as_ref()));
    match snapshot {
        Ok(snapshot) => {
            ShellStateEnvelope::new_v2(session_id, incarnation, sequence, cwd, snapshot)
        }
        Err(_) => ShellStateEnvelope::new(session_id, incarnation, sequence, cwd),
    }
}

fn parse_policy(environment: &[(OsString, OsString)]) -> Option<EnvironmentPolicy> {
    let include = control(environment, STATE_ENV_INCLUDE)?;
    let max_variables = control(environment, STATE_ENV_MAX_VARIABLES)?
        .parse()
        .ok()?;
    let max_bytes = control(environment, STATE_ENV_MAX_BYTES)?.parse().ok()?;
    let expected_fingerprint = control(environment, STATE_ENV_POLICY_FINGERPRINT)?;
    let include = if include.is_empty() {
        Vec::new()
    } else {
        include.split(',').map(str::to_owned).collect()
    };
    let policy = EnvironmentPolicy::new(&include, max_variables, max_bytes).ok()?;
    (policy.fingerprint() == expected_fingerprint).then_some(policy)
}

fn collect_environment(
    environment: &[(OsString, OsString)],
    policy: &EnvironmentPolicy,
) -> Result<BTreeMap<String, String>> {
    let mut current = BTreeMap::new();
    for (name, value) in environment {
        let Some(name) = name.to_str() else {
            continue;
        };
        if !policy.allows(name) {
            continue;
        }
        let value = value
            .to_str()
            .ok_or_else(|| invalid("allowed shell environment value is not UTF-8"))?;
        if current.insert(name.to_owned(), value.to_owned()).is_some() {
            return Err(invalid("allowed shell environment name is duplicated"));
        }
    }
    Ok(current)
}

fn read_cwd<R: Read>(reader: R) -> Result<String> {
    let mut cwd = Vec::with_capacity(MAX_SHELL_CWD_BYTES + 1);
    reader
        .take((MAX_SHELL_CWD_BYTES + 1) as u64)
        .read_to_end(&mut cwd)
        .map_err(|source| PersistError::Io {
            operation: "read shell state cwd",
            source,
        })?;
    if cwd.len() > MAX_SHELL_CWD_BYTES {
        return Err(invalid("shell state cwd exceeds size limit"));
    }
    String::from_utf8(cwd).map_err(|_| invalid("shell state cwd is not UTF-8"))
}

fn required_control(environment: &[(OsString, OsString)], name: &str) -> Result<String> {
    control(environment, name)
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("missing {name}")))
}

fn control<'a>(environment: &'a [(OsString, OsString)], name: &str) -> Option<&'a str> {
    environment
        .iter()
        .find(|(key, _)| key == OsStr::new(name))
        .and_then(|(_, value)| value.to_str())
}

fn parse_incarnation(encoded: &str) -> Result<[u8; 16]> {
    if encoded.len() != 32 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("invalid shell state incarnation"));
    }
    let mut value = [0u8; 16];
    for (index, byte) in value.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = (hex(encoded.as_bytes()[offset])? << 4) | hex(encoded.as_bytes()[offset + 1])?;
    }
    Ok(value)
}

fn hex(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(invalid("invalid shell state incarnation")),
    }
}

fn invalid(message: impl Into<String>) -> PersistError {
    PersistError::invalid_argument(message)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::io::Cursor;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;

    use persist_core::shell_state::{
        read_validated, EnvironmentCaptureStatus, EnvironmentPolicy, ShellStateEnvelope,
        ShellStateIdentity,
    };

    use super::commit_from_reader;

    #[test]
    fn helper_rejects_missing_invalid_and_oversized_input() {
        assert!(commit_from_reader(Vec::<(String, String)>::new(), Cursor::new(b"/srv")).is_err());

        let temp = tempfile::tempdir().unwrap();
        make_private(temp.path());
        let identity = persist_core::shell_state::create_identity(temp.path(), 7).unwrap();
        let mut environment = environment(&identity, "1");
        environment.insert("PERSIST_STATE_SESSION_ID".into(), "bad".into());
        assert!(commit_from_reader(environment.clone(), Cursor::new(b"/srv")).is_err());

        environment.insert("PERSIST_STATE_SESSION_ID".into(), "7".into());
        environment.insert("PERSIST_STATE_INCARNATION".into(), "xyz".into());
        assert!(commit_from_reader(environment.clone(), Cursor::new(b"/srv")).is_err());

        environment.insert(
            "PERSIST_STATE_INCARNATION".into(),
            identity.incarnation_hex(),
        );
        environment.insert("PERSIST_STATE_SEQUENCE".into(), "0".into());
        assert!(commit_from_reader(environment.clone(), Cursor::new(b"/srv")).is_err());

        environment.insert("PERSIST_STATE_SEQUENCE".into(), "1".into());
        environment.insert("PERSIST_STATE_FILE".into(), "relative.json".into());
        assert!(commit_from_reader(environment.clone(), Cursor::new(b"/srv")).is_err());

        environment.insert("PERSIST_STATE_FILE".into(), identity.path_string());
        assert!(commit_from_reader(environment, Cursor::new(vec![b'x'; 4097])).is_err());
    }

    #[test]
    fn helper_writes_valid_state_without_output_contract() {
        let temp = tempfile::tempdir().unwrap();
        make_private(temp.path());
        let identity = persist_core::shell_state::create_identity(temp.path(), 7).unwrap();
        commit_from_reader(environment(&identity, "3"), Cursor::new(b"/srv/final")).unwrap();

        assert_eq!(
            read_validated(&identity, 3).unwrap(),
            Some(
                ShellStateEnvelope::new(7, identity.incarnation(), 3, "/srv/final".into()).unwrap()
            )
        );
    }

    #[test]
    fn helper_captures_dynamic_environment_and_precise_unset() {
        let temp = tempfile::tempdir().unwrap();
        make_private(temp.path());
        let identity = persist_core::shell_state::create_identity(temp.path(), 7).unwrap();
        let mut first = environment_with_policy(&identity, "1", &["EDITOR", "MY_*"]);
        first.extend([
            ("LANG".into(), "C.UTF-8".into()),
            ("EDITOR".into(), "vim".into()),
            ("MY_OLD".into(), "old".into()),
            ("API_TOKEN".into(), "forbidden".into()),
            ("PERSIST_PRIVATE".into(), "forbidden".into()),
        ]);
        commit_from_reader(first, Cursor::new(b"/srv/one")).unwrap();

        let mut second = environment_with_policy(&identity, "2", &["EDITOR", "MY_*"]);
        second.extend([
            ("LANG".into(), "C.UTF-8".into()),
            ("EDITOR".into(), "nvim".into()),
            ("MY_NEW".into(), "new".into()),
        ]);
        commit_from_reader(second, Cursor::new(b"/srv/two")).unwrap();

        let state = read_validated(&identity, 2).unwrap().expect("state");
        let snapshot = state.environment.expect("environment");
        assert_eq!(state.cwd, "/srv/two");
        assert_eq!(
            snapshot.env_set.get("EDITOR").map(String::as_str),
            Some("nvim")
        );
        assert_eq!(
            snapshot.env_set.get("MY_NEW").map(String::as_str),
            Some("new")
        );
        assert!(snapshot.env_unset.contains("MY_OLD"));
        assert!(!snapshot.env_set.contains_key("API_TOKEN"));
        assert!(!snapshot.env_set.contains_key("PERSIST_PRIVATE"));
    }

    #[test]
    fn helper_preserves_environment_when_capture_fails_but_updates_cwd() {
        let temp = tempfile::tempdir().unwrap();
        make_private(temp.path());
        let identity = persist_core::shell_state::create_identity(temp.path(), 7).unwrap();
        let mut first = environment_with_policy(&identity, "1", &["EDITOR"]);
        first.insert("EDITOR".into(), "vim".into());
        commit_from_reader(first, Cursor::new(b"/srv/one")).unwrap();

        let mut second = environment_with_policy(&identity, "2", &["EDITOR"]);
        second.insert("EDITOR".into(), "x".repeat(8 * 1024 + 1));
        commit_from_reader(second, Cursor::new(b"/srv/two")).unwrap();

        let state = read_validated(&identity, 2).unwrap().expect("state");
        let snapshot = state.environment.expect("environment");
        assert_eq!(state.cwd, "/srv/two");
        assert_eq!(
            snapshot.env_set.get("EDITOR").map(String::as_str),
            Some("vim")
        );
        assert_eq!(snapshot.capture_status, EnvironmentCaptureStatus::Preserved);

        let mut third = environment_with_policy(&identity, "3", &["MY_*"]);
        for index in 0..129 {
            third.insert(format!("MY_{index}"), "v".into());
        }
        commit_from_reader(third, Cursor::new(b"/srv/three")).unwrap();
        let state = read_validated(&identity, 3).unwrap().expect("state");
        assert_eq!(state.cwd, "/srv/three");
        assert_eq!(
            state.environment.expect("environment").capture_status,
            EnvironmentCaptureStatus::Unavailable
        );
    }

    #[test]
    fn helper_handles_non_utf8_allowed_value_without_panicking() {
        let temp = tempfile::tempdir().unwrap();
        make_private(temp.path());
        let identity = persist_core::shell_state::create_identity(temp.path(), 7).unwrap();
        let strings = environment_with_policy(&identity, "1", &["EDITOR"]);
        let mut environment = strings
            .into_iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect::<Vec<_>>();
        environment.push((
            OsString::from("EDITOR"),
            OsString::from_vec(vec![0xff, 0xfe]),
        ));

        commit_from_reader(environment, Cursor::new(b"/srv/final")).unwrap();

        let state = read_validated(&identity, 1).unwrap().expect("state");
        assert_eq!(state.cwd, "/srv/final");
        assert_eq!(
            state.environment.expect("environment").capture_status,
            EnvironmentCaptureStatus::Unavailable
        );
    }

    #[test]
    fn helper_preserves_previous_environment_when_policy_hint_is_removed() {
        let temp = tempfile::tempdir().unwrap();
        make_private(temp.path());
        let identity = persist_core::shell_state::create_identity(temp.path(), 7).unwrap();
        let mut first = environment_with_policy(&identity, "1", &["EDITOR"]);
        first.insert("EDITOR".into(), "vim".into());
        commit_from_reader(first, Cursor::new(b"/srv/one")).unwrap();

        commit_from_reader(environment(&identity, "2"), Cursor::new(b"/srv/two")).unwrap();

        let state = read_validated(&identity, 2).unwrap().expect("state");
        let snapshot = state.environment.expect("environment");
        assert_eq!(state.cwd, "/srv/two");
        assert_eq!(
            snapshot.env_set.get("EDITOR").map(String::as_str),
            Some("vim")
        );
        assert_eq!(snapshot.capture_status, EnvironmentCaptureStatus::Preserved);
    }

    fn environment(identity: &ShellStateIdentity, sequence: &str) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("PERSIST_STATE_FILE".into(), identity.path_string()),
            ("PERSIST_STATE_SESSION_ID".into(), "7".into()),
            (
                "PERSIST_STATE_INCARNATION".into(),
                identity.incarnation_hex(),
            ),
            ("PERSIST_STATE_SEQUENCE".into(), sequence.into()),
        ])
    }

    fn environment_with_policy(
        identity: &ShellStateIdentity,
        sequence: &str,
        include: &[&str],
    ) -> BTreeMap<String, String> {
        let include = include
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        let policy = EnvironmentPolicy::new(&include, 128, 64 * 1024).unwrap();
        let mut environment = environment(identity, sequence);
        environment.extend([
            ("PERSIST_STATE_ENV_INCLUDE".into(), include.join(",")),
            ("PERSIST_STATE_ENV_MAX_VARIABLES".into(), "128".into()),
            (
                "PERSIST_STATE_ENV_MAX_BYTES".into(),
                (64 * 1024).to_string(),
            ),
            (
                "PERSIST_STATE_ENV_POLICY_FINGERPRINT".into(),
                policy.fingerprint().into(),
            ),
        ]);
        environment
    }

    fn make_private(path: &std::path::Path) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}
