use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{PersistError, Result};

mod launch;
mod unique;
pub use launch::ShellLaunchEnvironment;

pub const SHELL_ENVIRONMENT_FORMAT_VERSION: u32 = 1;
pub const SHELL_ENVIRONMENT_POLICY_VERSION: u32 = 1;
pub const MAX_SHELL_ENVIRONMENT_VARIABLES: usize = 128;
pub const MAX_SHELL_ENVIRONMENT_NAME_BYTES: usize = 128;
pub const MAX_SHELL_ENVIRONMENT_VALUE_BYTES: usize = 8 * 1024;
pub const MAX_SHELL_ENVIRONMENT_BYTES: usize = 64 * 1024;
pub const STATE_ENV_INCLUDE: &str = "PERSIST_STATE_ENV_INCLUDE";
pub const STATE_ENV_MAX_VARIABLES: &str = "PERSIST_STATE_ENV_MAX_VARIABLES";
pub const STATE_ENV_MAX_BYTES: &str = "PERSIST_STATE_ENV_MAX_BYTES";
pub const STATE_ENV_POLICY_FINGERPRINT: &str = "PERSIST_STATE_ENV_POLICY_FINGERPRINT";

const CONNECTION_NAMES: &[&str] = &[
    "COLORTERM",
    "DISPLAY",
    "SSH_AUTH_SOCK",
    "SSH_CLIENT",
    "SSH_CONNECTION",
    "SSH_TTY",
    "TERM",
    "WAYLAND_DISPLAY",
];
const BASE_NAMES: &[&str] = &[
    "HOME", "LOGNAME", "OLDPWD", "PATH", "PWD", "SHELL", "SHLVL", "USER", "_",
];
const SENSITIVE_MARKERS: &[&str] = &[
    "ACCESS_KEY",
    "API_KEY",
    "COOKIE",
    "CREDENTIAL",
    "PASSWORD",
    "PASSWD",
    "PRIVATE_KEY",
    "SECRET",
    "TOKEN",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentCaptureStatus {
    Complete,
    Preserved,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentSnapshot {
    pub format_version: u32,
    pub policy_version: u32,
    pub policy_fingerprint: String,
    #[serde(deserialize_with = "unique::map")]
    pub env_set: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique::set")]
    pub env_unset: BTreeSet<String>,
    pub capture_status: EnvironmentCaptureStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentPolicy {
    include: Vec<String>,
    max_variables: usize,
    max_bytes: usize,
    fingerprint: String,
}

impl EnvironmentPolicy {
    pub fn new(include: &[String], max_variables: usize, max_bytes: usize) -> Result<Self> {
        if max_variables == 0 || max_variables > MAX_SHELL_ENVIRONMENT_VARIABLES {
            return Err(invalid("environment variable limit is invalid"));
        }
        if max_bytes == 0 || max_bytes > MAX_SHELL_ENVIRONMENT_BYTES {
            return Err(invalid("environment byte limit is invalid"));
        }
        let mut normalized = include.to_vec();
        normalized.sort();
        if normalized.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid("environment include rules contain duplicates"));
        }
        for rule in &normalized {
            validate_rule(rule)?;
        }
        let required_names = std::iter::once("LANG")
            .chain(
                normalized
                    .iter()
                    .filter(|rule| !rule.ends_with('*'))
                    .map(String::as_str),
            )
            .collect::<BTreeSet<_>>();
        if required_names.len() > max_variables {
            return Err(invalid(
                "environment variable limit is below exact include count",
            ));
        }
        let fingerprint = fingerprint(&normalized, max_variables, max_bytes);
        Ok(Self {
            include: normalized,
            max_variables,
            max_bytes,
            fingerprint,
        })
    }

    pub fn allows(&self, name: &str) -> bool {
        if !valid_name(name) || hard_denied(name) {
            return false;
        }
        name == "LANG"
            || name.starts_with("LC_")
            || self.include.iter().any(|rule| rule_matches(rule, name))
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn filter_snapshot(&self, snapshot: &EnvironmentSnapshot) -> Result<EnvironmentSnapshot> {
        snapshot.validate_hard_limits()?;
        let env_set = snapshot
            .env_set
            .iter()
            .filter(|(name, _)| self.allows(name))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        let env_unset = snapshot
            .env_unset
            .iter()
            .filter(|name| self.allows(name))
            .cloned()
            .collect();
        let changed = snapshot.policy_fingerprint != self.fingerprint
            || env_set != snapshot.env_set
            || env_unset != snapshot.env_unset;
        let filtered = EnvironmentSnapshot {
            format_version: SHELL_ENVIRONMENT_FORMAT_VERSION,
            policy_version: SHELL_ENVIRONMENT_POLICY_VERSION,
            policy_fingerprint: self.fingerprint.clone(),
            env_set,
            env_unset,
            capture_status: if changed {
                EnvironmentCaptureStatus::Preserved
            } else {
                snapshot.capture_status
            },
        };
        filtered.validate(self.max_variables, self.max_bytes)?;
        Ok(filtered)
    }

    fn exact_names(&self) -> impl Iterator<Item = &str> {
        std::iter::once("LANG").chain(
            self.include
                .iter()
                .filter(|rule| !rule.ends_with('*'))
                .map(String::as_str),
        )
    }
}

impl EnvironmentSnapshot {
    pub fn capture(
        policy: &EnvironmentPolicy,
        previous: Option<&Self>,
        current: BTreeMap<String, String>,
    ) -> Result<Self> {
        let env_set = current
            .into_iter()
            .filter(|(name, _)| policy.allows(name))
            .collect::<BTreeMap<_, _>>();
        let mut tracked = policy
            .exact_names()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        if let Some(previous) = previous {
            tracked.extend(previous.env_set.keys().cloned());
            tracked.extend(previous.env_unset.iter().cloned());
        }
        let env_unset = tracked
            .into_iter()
            .filter(|name| policy.allows(name) && !env_set.contains_key(name))
            .collect();
        let snapshot = Self {
            format_version: SHELL_ENVIRONMENT_FORMAT_VERSION,
            policy_version: SHELL_ENVIRONMENT_POLICY_VERSION,
            policy_fingerprint: policy.fingerprint.clone(),
            env_set,
            env_unset,
            capture_status: EnvironmentCaptureStatus::Complete,
        };
        snapshot.validate(policy.max_variables, policy.max_bytes)?;
        Ok(snapshot)
    }

    pub fn fallback(policy: &EnvironmentPolicy, previous: Option<&Self>) -> Result<Self> {
        if let Some(previous) = previous.filter(|snapshot| {
            snapshot.policy_fingerprint == policy.fingerprint
                && (!snapshot.env_set.is_empty() || !snapshot.env_unset.is_empty())
        }) {
            let mut preserved = previous.clone();
            preserved.capture_status = EnvironmentCaptureStatus::Preserved;
            if preserved
                .validate(policy.max_variables, policy.max_bytes)
                .is_ok()
            {
                return Ok(preserved);
            }
        }
        let unavailable = Self {
            format_version: SHELL_ENVIRONMENT_FORMAT_VERSION,
            policy_version: SHELL_ENVIRONMENT_POLICY_VERSION,
            policy_fingerprint: policy.fingerprint.clone(),
            env_set: BTreeMap::new(),
            env_unset: BTreeSet::new(),
            capture_status: EnvironmentCaptureStatus::Unavailable,
        };
        unavailable.validate(policy.max_variables, policy.max_bytes)?;
        Ok(unavailable)
    }

    pub fn preserved(&self) -> Result<Self> {
        if self.env_set.is_empty() && self.env_unset.is_empty() {
            return Err(invalid("empty environment snapshot cannot be preserved"));
        }
        let mut preserved = self.clone();
        preserved.capture_status = EnvironmentCaptureStatus::Preserved;
        preserved.validate_hard_limits()?;
        Ok(preserved)
    }

    pub fn validate_hard_limits(&self) -> Result<()> {
        self.validate(MAX_SHELL_ENVIRONMENT_VARIABLES, MAX_SHELL_ENVIRONMENT_BYTES)
    }

    fn validate(&self, max_variables: usize, max_bytes: usize) -> Result<()> {
        if self.format_version != SHELL_ENVIRONMENT_FORMAT_VERSION
            || self.policy_version != SHELL_ENVIRONMENT_POLICY_VERSION
            || !valid_fingerprint(&self.policy_fingerprint)
        {
            return Err(invalid(
                "environment snapshot version or fingerprint is invalid",
            ));
        }
        if self.env_set.len() + self.env_unset.len() > max_variables {
            return Err(invalid("environment snapshot has too many variables"));
        }
        if self
            .env_set
            .keys()
            .any(|name| self.env_unset.contains(name))
        {
            return Err(invalid("environment snapshot set and unset overlap"));
        }
        for (name, value) in &self.env_set {
            validate_entry(name, value)?;
        }
        for name in &self.env_unset {
            validate_entry(name, "")?;
        }
        if self.capture_status == EnvironmentCaptureStatus::Unavailable
            && (!self.env_set.is_empty() || !self.env_unset.is_empty())
        {
            return Err(invalid("unavailable environment snapshot contains values"));
        }
        let encoded = serde_json::to_vec(self)
            .map_err(|error| invalid(format!("encode environment snapshot: {error}")))?;
        if encoded.len() > max_bytes {
            return Err(invalid("environment snapshot exceeds size limit"));
        }
        Ok(())
    }
}

pub fn encode_environment_snapshot(snapshot: &EnvironmentSnapshot) -> Result<Vec<u8>> {
    snapshot.validate_hard_limits()?;
    let encoded = serde_json::to_vec(snapshot)
        .map_err(|error| invalid(format!("encode environment snapshot: {error}")))?;
    if encoded.len() > MAX_SHELL_ENVIRONMENT_BYTES {
        return Err(invalid("environment snapshot exceeds size limit"));
    }
    Ok(encoded)
}

pub fn decode_environment_snapshot(encoded: &[u8]) -> Result<EnvironmentSnapshot> {
    if encoded.len() > MAX_SHELL_ENVIRONMENT_BYTES {
        return Err(invalid("environment snapshot exceeds size limit"));
    }
    let snapshot: EnvironmentSnapshot = serde_json::from_slice(encoded)
        .map_err(|error| invalid(format!("decode environment snapshot: {error}")))?;
    snapshot.validate_hard_limits()?;
    Ok(snapshot)
}

fn validate_rule(rule: &str) -> Result<()> {
    let name = rule.strip_suffix('*').unwrap_or(rule);
    if name.is_empty()
        || rule.matches('*').count() > usize::from(rule.ends_with('*'))
        || !valid_name(name)
        || hard_denied(name)
    {
        return Err(invalid("environment include rule is invalid or forbidden"));
    }
    Ok(())
}

fn rule_matches(rule: &str, name: &str) -> bool {
    rule.strip_suffix('*')
        .map_or_else(|| rule == name, |prefix| name.starts_with(prefix))
}

fn valid_name(name: &str) -> bool {
    name.len() <= MAX_SHELL_ENVIRONMENT_NAME_BYTES
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn hard_denied(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    CONNECTION_NAMES.contains(&upper.as_str())
        || BASE_NAMES.contains(&upper.as_str())
        || upper.starts_with("XDG_")
        || upper.starts_with("PERSIST_")
        || SENSITIVE_MARKERS
            .iter()
            .any(|marker| upper.contains(marker))
}

fn validate_entry(name: &str, value: &str) -> Result<()> {
    if !valid_name(name) || value.len() > MAX_SHELL_ENVIRONMENT_VALUE_BYTES || value.contains('\0')
    {
        return Err(invalid("environment snapshot entry is invalid"));
    }
    Ok(())
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn fingerprint(include: &[String], max_variables: usize, max_bytes: usize) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    let max_variables = u64::try_from(max_variables).expect("validated variable limit fits u64");
    let max_bytes = u64::try_from(max_bytes).expect("validated byte limit fits u64");
    for byte in SHELL_ENVIRONMENT_POLICY_VERSION
        .to_be_bytes()
        .into_iter()
        .chain(max_variables.to_be_bytes())
        .chain(max_bytes.to_be_bytes())
        .chain(include.iter().flat_map(|rule| rule.bytes().chain([0])))
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn invalid(message: impl Into<String>) -> PersistError {
    PersistError::invalid_argument(message)
}
