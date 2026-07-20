use persist_ipc::holder::{HolderLogState, HolderSessionEntry, HolderSessionState};
use persist_metadata::MetadataStore;
use std::collections::{BTreeMap, BTreeSet};

use super::*;

fn policy() -> persist_core::shell_state::EnvironmentPolicy {
    persist_core::shell_state::EnvironmentPolicy::new(&[], 128, 64 * 1024).unwrap()
}

fn entry(session_id: u32, state: HolderSessionState, exit_code: Option<i32>) -> HolderSessionEntry {
    HolderSessionEntry {
        session_id,
        shell_pid: std::process::id(),
        state,
        exit_code,
        created_at_ms: 1,
        last_active_at_ms: 2,
        ring_bytes: 0,
        writer_active: false,
        log_state: HolderLogState::Healthy,
        exit_context_available: false,
    }
}

#[test]
fn reconcile_maps_running_exited_missing_and_orphan() {
    let mut metadata = MetadataStore::open_in_memory().unwrap();
    for id in 1..=3 {
        metadata
            .create_session(id, &format!("s{id}"), None, None)
            .unwrap();
    }
    metadata.close_session(3, 0).unwrap();
    let snapshot = HolderInventorySnapshot {
        instance_id: [0x11; 16],
        generation: 9,
        entries: vec![
            entry(1, HolderSessionState::Running, None),
            entry(2, HolderSessionState::Exited, Some(17)),
            entry(4, HolderSessionState::Running, None),
        ],
    };

    let result = reconcile_metadata(&mut metadata, &snapshot, &HashMap::new(), &policy()).unwrap();
    assert_eq!(metadata.get_session(1).unwrap().unwrap().status, "running");
    let exited = metadata.get_session(2).unwrap().unwrap();
    assert_eq!(exited.status, "closed");
    assert_eq!(exited.exit_code, Some(17));
    assert_eq!(metadata.get_session(3).unwrap().unwrap().status, "closed");
    assert_eq!(result.orphaned_sessions, HashSet::from([4]));

    let repeated =
        reconcile_metadata(&mut metadata, &snapshot, &HashMap::new(), &policy()).unwrap();
    assert_eq!(repeated.orphaned_sessions, HashSet::from([4]));
}

#[test]
fn queried_exit_context_updates_cwd_before_retire() {
    let mut metadata = MetadataStore::open_in_memory().unwrap();
    metadata
        .create_session(2, "exited", Some("/old"), None)
        .unwrap();
    let mut exited = entry(2, HolderSessionState::Exited, Some(17));
    exited.exit_context_available = true;
    let snapshot = HolderInventorySnapshot {
        instance_id: [0x11; 16],
        generation: 9,
        entries: vec![exited],
    };
    let contexts = HashMap::from([(
        2,
        ExitContext {
            session_id: 2,
            exit_code: 23,
            cwd: Some("/srv/final".into()),
            environment: None,
        },
    )]);

    let result = reconcile_metadata(&mut metadata, &snapshot, &contexts, &policy()).unwrap();
    assert_eq!(result.exited_sessions, vec![2]);
    let record = metadata.get_session(2).unwrap().unwrap();
    assert_eq!(record.exit_code, Some(23));
    assert_eq!(record.cwd.as_deref(), Some("/srv/final"));
    assert!(reconcile_metadata(&mut metadata, &snapshot, &HashMap::new(), &policy()).is_err());
}

#[test]
fn queried_exit_context_persists_v2_environment() {
    let mut metadata = MetadataStore::open_in_memory().unwrap();
    metadata.create_session(5, "exited", None, None).unwrap();
    let mut exited = entry(5, HolderSessionState::Exited, Some(9));
    exited.exit_context_available = true;
    let snapshot = HolderInventorySnapshot {
        instance_id: [0x33; 16],
        generation: 4,
        entries: vec![exited],
    };
    let policy = policy();
    let environment = persist_core::shell_state::EnvironmentSnapshot {
        format_version: persist_core::shell_state::SHELL_ENVIRONMENT_FORMAT_VERSION,
        policy_version: persist_core::shell_state::SHELL_ENVIRONMENT_POLICY_VERSION,
        policy_fingerprint: policy.fingerprint().to_owned(),
        env_set: BTreeMap::from([("LANG".to_owned(), "C.UTF-8".to_owned())]),
        env_unset: BTreeSet::new(),
        capture_status: persist_core::shell_state::EnvironmentCaptureStatus::Complete,
    };
    let contexts = HashMap::from([(
        5,
        ExitContext {
            session_id: 5,
            exit_code: 9,
            cwd: None,
            environment: Some(environment.clone()),
        },
    )]);

    reconcile_metadata(&mut metadata, &snapshot, &contexts, &policy).unwrap();
    let record = metadata.get_session(5).unwrap().unwrap();
    assert_eq!(
        persist_metadata::decode_environment(record.env_snapshot.as_deref(), &policy).unwrap(),
        Some(environment)
    );
}

#[test]
fn missing_active_metadata_becomes_lost() {
    let mut metadata = MetadataStore::open_in_memory().unwrap();
    metadata.create_session(7, "missing", None, None).unwrap();
    let snapshot = HolderInventorySnapshot {
        instance_id: [0x22; 16],
        generation: 3,
        entries: Vec::new(),
    };
    reconcile_metadata(&mut metadata, &snapshot, &HashMap::new(), &policy()).unwrap();
    assert_eq!(metadata.get_session(7).unwrap().unwrap().status, "lost");
}

#[test]
fn conflicting_holder_instance_is_isolated() {
    let mut metadata = MetadataStore::open_in_memory().unwrap();
    metadata.create_session(8, "conflict", None, None).unwrap();
    metadata
        .reconcile_running(8, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 1)
        .unwrap();
    let snapshot = HolderInventorySnapshot {
        instance_id: [0xbb; 16],
        generation: 2,
        entries: vec![entry(8, HolderSessionState::Running, None)],
    };
    let result = reconcile_metadata(&mut metadata, &snapshot, &HashMap::new(), &policy()).unwrap();
    assert_eq!(metadata.get_session(8).unwrap().unwrap().status, "lost");
    assert!(result.orphaned_sessions.contains(&8));
}
