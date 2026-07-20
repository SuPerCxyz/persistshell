use std::collections::BTreeMap;
use std::fmt;

use persist_core::shell_state::{
    decode_environment_snapshot, encode_environment_snapshot, EnvironmentCaptureStatus,
    EnvironmentPolicy, EnvironmentSnapshot, SHELL_ENVIRONMENT_FORMAT_VERSION,
    SHELL_ENVIRONMENT_POLICY_VERSION,
};
use persist_core::{PersistError, Result};
use serde::de::{MapAccess, Visitor};
use serde::Deserializer;

pub fn encode_environment(
    snapshot: &EnvironmentSnapshot,
    policy: &EnvironmentPolicy,
) -> Result<String> {
    let filtered = policy.filter_snapshot(snapshot)?;
    let encoded = encode_environment_snapshot(&filtered)?;
    String::from_utf8(encoded)
        .map_err(|_| PersistError::invalid_argument("environment metadata is not UTF-8"))
}

pub fn decode_environment(
    encoded: Option<&str>,
    policy: &EnvironmentPolicy,
) -> Result<Option<EnvironmentSnapshot>> {
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    if encoded.len() > persist_core::shell_state::MAX_SHELL_ENVIRONMENT_BYTES {
        return Err(PersistError::invalid_argument(
            "environment metadata exceeds size limit",
        ));
    }
    if let Ok(snapshot) = decode_environment_snapshot(encoded.as_bytes()) {
        return policy.filter_snapshot(&snapshot).map(Some);
    }
    let legacy = decode_legacy_map(encoded)?;
    let snapshot = EnvironmentSnapshot {
        format_version: SHELL_ENVIRONMENT_FORMAT_VERSION,
        policy_version: SHELL_ENVIRONMENT_POLICY_VERSION,
        policy_fingerprint: policy.fingerprint().to_owned(),
        env_set: legacy,
        env_unset: Default::default(),
        capture_status: EnvironmentCaptureStatus::Preserved,
    };
    policy.filter_snapshot(&snapshot).map(Some)
}

fn decode_legacy_map(encoded: &str) -> Result<BTreeMap<String, String>> {
    let mut deserializer = serde_json::Deserializer::from_str(encoded);
    let map = deserializer
        .deserialize_map(UniqueStringMap)
        .map_err(|_| PersistError::invalid_argument("invalid legacy environment metadata"))?;
    deserializer
        .end()
        .map_err(|_| PersistError::invalid_argument("invalid legacy environment metadata"))?;
    Ok(map)
}

struct UniqueStringMap;

impl<'de> Visitor<'de> for UniqueStringMap {
    type Value = BTreeMap<String, String>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map with unique string keys and string values")
    }

    fn visit_map<A>(self, mut access: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some((name, value)) = access.next_entry::<String, String>()? {
            if values.insert(name, value).is_some() {
                return Err(serde::de::Error::custom(
                    "duplicate legacy environment name",
                ));
            }
        }
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    fn policy(include: &[&str]) -> EnvironmentPolicy {
        EnvironmentPolicy::new(
            &include
                .iter()
                .map(|item| (*item).to_owned())
                .collect::<Vec<_>>(),
            128,
            64 * 1024,
        )
        .unwrap()
    }

    #[test]
    fn legacy_map_is_filtered_and_converted() {
        let decoded = decode_environment(
            Some(r#"{"EDITOR":"vim","LANG":"C","PATH":"/bad","TOKEN":"bad"}"#),
            &policy(&["EDITOR"]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            decoded.env_set,
            BTreeMap::from([
                ("EDITOR".to_owned(), "vim".to_owned()),
                ("LANG".to_owned(), "C".to_owned()),
            ])
        );
        assert_eq!(decoded.capture_status, EnvironmentCaptureStatus::Preserved);
    }

    #[test]
    fn v2_round_trip_preserves_set_and_unset() {
        let policy = policy(&["EDITOR"]);
        let snapshot = EnvironmentSnapshot {
            format_version: SHELL_ENVIRONMENT_FORMAT_VERSION,
            policy_version: SHELL_ENVIRONMENT_POLICY_VERSION,
            policy_fingerprint: policy.fingerprint().to_owned(),
            env_set: BTreeMap::from([("LANG".to_owned(), "C.UTF-8".to_owned())]),
            env_unset: BTreeSet::from(["EDITOR".to_owned()]),
            capture_status: EnvironmentCaptureStatus::Complete,
        };
        let encoded = encode_environment(&snapshot, &policy).unwrap();
        assert_eq!(
            decode_environment(Some(&encoded), &policy).unwrap(),
            Some(snapshot)
        );
    }

    #[test]
    fn duplicate_legacy_name_is_rejected() {
        assert!(
            decode_environment(Some(r#"{"LANG":"C","LANG":"C.UTF-8"}"#), &policy(&[])).is_err()
        );
    }

    #[test]
    fn tighter_policy_removes_revoked_names() {
        let broad = policy(&["EDITOR", "MY_*"]);
        let snapshot = EnvironmentSnapshot {
            format_version: SHELL_ENVIRONMENT_FORMAT_VERSION,
            policy_version: SHELL_ENVIRONMENT_POLICY_VERSION,
            policy_fingerprint: broad.fingerprint().to_owned(),
            env_set: BTreeMap::from([
                ("EDITOR".to_owned(), "vim".to_owned()),
                ("MY_FLAG".to_owned(), "1".to_owned()),
            ]),
            env_unset: BTreeSet::new(),
            capture_status: EnvironmentCaptureStatus::Complete,
        };
        let encoded = encode_environment(&snapshot, &broad).unwrap();
        let filtered = decode_environment(Some(&encoded), &policy(&["EDITOR"]))
            .unwrap()
            .unwrap();
        assert!(!filtered.env_set.contains_key("MY_FLAG"));
        assert_eq!(filtered.capture_status, EnvironmentCaptureStatus::Preserved);
    }

    #[test]
    fn malformed_or_conflicting_v2_is_rejected() {
        let policy = policy(&["EDITOR"]);
        let conflicting = format!(
            r#"{{"format_version":1,"policy_version":1,"policy_fingerprint":"{}","env_set":{{"EDITOR":"vim"}},"env_unset":["EDITOR"],"capture_status":"complete"}}"#,
            policy.fingerprint()
        );
        assert!(decode_environment(Some(&conflicting), &policy).is_err());
        assert!(decode_environment(Some(r#"{"LANG":1}"#), &policy).is_err());
    }
}
