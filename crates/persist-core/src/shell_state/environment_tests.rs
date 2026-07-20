use std::collections::BTreeMap;

use super::{
    EnvironmentCaptureStatus, EnvironmentPolicy, EnvironmentSnapshot, ShellLaunchEnvironment,
    ShellStateEnvelope, MAX_SHELL_ENVIRONMENT_BYTES, MAX_SHELL_ENVIRONMENT_VALUE_BYTES,
    MAX_SHELL_ENVIRONMENT_VARIABLES,
};

const INCARNATION: [u8; 16] = [0x44; 16];

#[test]
fn launch_environment_enforces_shared_source_and_resource_boundaries() {
    assert!(ShellLaunchEnvironment::new(
        vec![("M55_DUP".into(), "one".into())],
        vec![],
        vec![],
        vec![("M55_DUP".into(), "two".into())],
    )
    .is_err());
    assert!(ShellLaunchEnvironment::new(
        vec![],
        vec![],
        vec![("UNKNOWN_CONNECTION".into(), "value".into())],
        vec![],
    )
    .is_err());
    assert!(ShellLaunchEnvironment::new(
        vec![(
            "M55_LARGE".into(),
            "x".repeat(MAX_SHELL_ENVIRONMENT_VALUE_BYTES + 1),
        )],
        vec![],
        vec![],
        vec![],
    )
    .is_err());

    let too_many = (0..=MAX_SHELL_ENVIRONMENT_VARIABLES)
        .map(|index| (format!("M55_{index}"), "value".into()))
        .collect();
    assert!(ShellLaunchEnvironment::new(too_many, vec![], vec![], vec![]).is_err());
}

#[test]
fn environment_policy_defaults_extensions_and_hard_denials_are_stable() {
    let policy = EnvironmentPolicy::new(&["EDITOR".into(), "MY_PROJECT_*".into()], 32, 16 * 1024)
        .expect("policy");

    assert!(policy.allows("LANG"));
    assert!(policy.allows("LC_ALL"));
    assert!(policy.allows("EDITOR"));
    assert!(policy.allows("MY_PROJECT_ROOT"));
    assert!(!policy.allows("TERM"));
    assert!(!policy.allows("PATH"));
    assert!(!policy.allows("PERSIST_STATE_FILE"));
    assert!(!policy.allows("MY_PROJECT_TOKEN"));
    assert_eq!(policy.fingerprint().len(), 16);

    let reordered =
        EnvironmentPolicy::new(&["MY_PROJECT_*".into(), "EDITOR".into()], 32, 16 * 1024)
            .expect("policy");
    assert_eq!(policy.fingerprint(), reordered.fingerprint());
    let tighter = EnvironmentPolicy::new(&["EDITOR".into(), "MY_PROJECT_*".into()], 31, 16 * 1024)
        .expect("policy");
    assert_ne!(policy.fingerprint(), tighter.fingerprint());
}

#[test]
fn environment_policy_rejects_invalid_duplicate_and_unsafe_rules() {
    for rules in [
        vec!["API_TOKEN".into()],
        vec!["PATH".into()],
        vec!["PERSIST_*".into()],
        vec!["MY_*_VALUE".into()],
        vec!["9INVALID".into()],
        vec!["变量".into()],
        vec!["EDITOR".into(), "EDITOR".into()],
    ] {
        assert!(EnvironmentPolicy::new(&rules, 128, 64 * 1024).is_err());
    }
    assert!(EnvironmentPolicy::new(&[], 0, 64 * 1024).is_err());
    assert!(EnvironmentPolicy::new(&[], 129, 64 * 1024).is_err());
    assert!(EnvironmentPolicy::new(&[], 128, 0).is_err());
    assert!(EnvironmentPolicy::new(&[], 128, 64 * 1024 + 1).is_err());
    assert!(EnvironmentPolicy::new(&["EDITOR".into()], 1, 64 * 1024).is_err());
}

#[test]
fn snapshot_tracks_updates_and_precise_unsets() {
    let policy =
        EnvironmentPolicy::new(&["EDITOR".into(), "MY_*".into()], 128, 64 * 1024).expect("policy");
    let previous = EnvironmentSnapshot::capture(
        &policy,
        None,
        BTreeMap::from([
            ("EDITOR".into(), "vim".into()),
            ("MY_OLD".into(), "old".into()),
        ]),
    )
    .expect("previous");
    let current = EnvironmentSnapshot::capture(
        &policy,
        Some(&previous),
        BTreeMap::from([
            ("EDITOR".into(), "nvim".into()),
            ("MY_NEW".into(), "new".into()),
            ("API_TOKEN".into(), "must-not-persist".into()),
        ]),
    )
    .expect("current");

    assert_eq!(
        current.env_set.get("EDITOR").map(String::as_str),
        Some("nvim")
    );
    assert_eq!(
        current.env_set.get("MY_NEW").map(String::as_str),
        Some("new")
    );
    assert!(current.env_unset.contains("MY_OLD"));
    assert!(!current.env_set.contains_key("API_TOKEN"));
    assert_eq!(current.capture_status, EnvironmentCaptureStatus::Complete);
}

#[test]
fn envelope_v2_round_trip_and_v1_compatibility_are_strict() {
    let policy = EnvironmentPolicy::new(&["EDITOR".into()], 128, 64 * 1024).expect("policy");
    let snapshot = EnvironmentSnapshot::capture(
        &policy,
        None,
        BTreeMap::from([("EDITOR".into(), "nvim".into())]),
    )
    .expect("snapshot");
    let state =
        ShellStateEnvelope::new_v2(7, INCARNATION, 3, "/srv/work".into(), snapshot).expect("v2");
    let encoded = super::encode_envelope(&state).expect("encode");
    let identity = super::identity_from_parts(
        7,
        INCARNATION,
        "/run/user/1000/persistshell/session-state/7-44444444444444444444444444444444.json".into(),
    )
    .expect("identity");

    assert_eq!(
        super::decode_and_validate(&identity, 2, &encoded).expect("decode"),
        state
    );
    assert!(ShellStateEnvelope::new(7, INCARNATION, 1, "/srv".into())
        .expect("v1")
        .environment
        .is_none());
}

#[test]
fn environment_snapshot_enforces_aggregate_size() {
    let policy =
        EnvironmentPolicy::new(&["BIG".into()], 128, MAX_SHELL_ENVIRONMENT_BYTES).expect("policy");
    let current = BTreeMap::from([("BIG".into(), "x".repeat(MAX_SHELL_ENVIRONMENT_BYTES))]);

    assert!(EnvironmentSnapshot::capture(&policy, None, current).is_err());
}

#[test]
fn environment_snapshot_enforces_count_name_and_value_limits() {
    let policy = EnvironmentPolicy::new(&["SAFE_*".into()], 128, 64 * 1024).expect("policy");
    let mut at_limit = (0..127)
        .map(|index| (format!("SAFE_{index}"), "v".to_string()))
        .collect::<BTreeMap<_, _>>();
    at_limit.insert("LANG".into(), "C".into());
    assert!(EnvironmentSnapshot::capture(&policy, None, at_limit).is_ok());

    let mut over_count = (0..128)
        .map(|index| (format!("SAFE_{index}"), "v".to_string()))
        .collect::<BTreeMap<_, _>>();
    over_count.insert("LANG".into(), "C".into());
    assert!(EnvironmentSnapshot::capture(&policy, None, over_count).is_err());

    let max_name = format!("S{}", "A".repeat(127));
    let max_name_policy =
        EnvironmentPolicy::new(std::slice::from_ref(&max_name), 128, 64 * 1024).expect("policy");
    assert!(EnvironmentSnapshot::capture(
        &max_name_policy,
        None,
        BTreeMap::from([(max_name, "v".into())]),
    )
    .is_ok());
    assert!(EnvironmentPolicy::new(&[format!("S{}", "A".repeat(128))], 128, 64 * 1024).is_err());

    let value_policy = EnvironmentPolicy::new(&["SAFE".into()], 128, 64 * 1024).expect("policy");
    assert!(EnvironmentSnapshot::capture(
        &value_policy,
        None,
        BTreeMap::from([("SAFE".into(), "x".repeat(8 * 1024))]),
    )
    .is_ok());
    assert!(EnvironmentSnapshot::capture(
        &value_policy,
        None,
        BTreeMap::from([("SAFE".into(), "x".repeat(8 * 1024 + 1))]),
    )
    .is_err());
}

#[test]
fn envelope_rejects_invalid_v2_combinations_and_snapshot_fields() {
    let policy = EnvironmentPolicy::new(&["EDITOR".into()], 128, 64 * 1024).expect("policy");
    let snapshot = EnvironmentSnapshot::capture(
        &policy,
        None,
        BTreeMap::from([("EDITOR".into(), "vim".into())]),
    )
    .expect("snapshot");
    let mut state =
        ShellStateEnvelope::new_v2(7, INCARNATION, 1, "/srv".into(), snapshot).expect("state");

    state
        .environment
        .as_mut()
        .expect("environment")
        .env_unset
        .insert("EDITOR".into());
    assert!(super::encode_envelope(&state).is_err());

    state
        .environment
        .as_mut()
        .expect("environment")
        .env_unset
        .clear();
    state.version = 1;
    assert!(super::encode_envelope(&state).is_err());

    state.version = 2;
    state.environment = None;
    assert!(super::encode_envelope(&state).is_err());

    let unknown = br#"{"version":2,"session_id":7,"incarnation":"44444444444444444444444444444444","sequence":1,"cwd":"/srv","environment":{"format_version":1,"policy_version":1,"policy_fingerprint":"0123456789abcdef","env_set":{},"env_unset":[],"capture_status":"complete","extra":1}}"#;
    let identity = super::identity_from_parts(
        7,
        INCARNATION,
        "/run/user/1000/persistshell/session-state/7-44444444444444444444444444444444.json".into(),
    )
    .expect("identity");
    assert!(super::decode_and_validate(&identity, 0, unknown).is_err());

    let duplicate_set = br#"{"version":2,"session_id":7,"incarnation":"44444444444444444444444444444444","sequence":1,"cwd":"/srv","environment":{"format_version":1,"policy_version":1,"policy_fingerprint":"0123456789abcdef","env_set":{"EDITOR":"vim","EDITOR":"nvim"},"env_unset":[],"capture_status":"complete"}}"#;
    assert!(super::decode_and_validate(&identity, 0, duplicate_set).is_err());

    let duplicate_unset = br#"{"version":2,"session_id":7,"incarnation":"44444444444444444444444444444444","sequence":1,"cwd":"/srv","environment":{"format_version":1,"policy_version":1,"policy_fingerprint":"0123456789abcdef","env_set":{},"env_unset":["EDITOR","EDITOR"],"capture_status":"complete"}}"#;
    assert!(super::decode_and_validate(&identity, 0, duplicate_unset).is_err());

    assert!(super::decode_and_validate(&identity, 0, &[0xff]).is_err());
    assert!(EnvironmentSnapshot::capture(
        &policy,
        None,
        BTreeMap::from([("EDITOR".into(), "bad\0value".into())]),
    )
    .is_err());
}
