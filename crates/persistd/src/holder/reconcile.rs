use std::collections::{HashMap, HashSet};

use persist_core::shell_state::EnvironmentPolicy;
use persist_core::Result;
use persist_ipc::holder::HolderSessionState;
use persist_metadata::{MetadataStore, SessionRecord};

use super::{ExitContext, HolderInventorySnapshot};

pub(crate) struct ReconciliationResult {
    pub(crate) orphaned_sessions: HashSet<u32>,
    pub(crate) exited_sessions: Vec<u32>,
}

pub(crate) fn reconcile_metadata(
    metadata: &mut MetadataStore,
    snapshot: &HolderInventorySnapshot,
    exit_contexts: &HashMap<u32, ExitContext>,
    environment_policy: &EnvironmentPolicy,
) -> Result<ReconciliationResult> {
    let instance = snapshot.instance_hex();
    let records = metadata
        .list_sessions()?
        .into_iter()
        .map(|record| (record.session_id, record))
        .collect::<HashMap<_, _>>();
    let holder_ids = snapshot
        .entries
        .iter()
        .map(|entry| entry.session_id)
        .collect::<HashSet<_>>();
    let mut orphaned_sessions = HashSet::new();
    let mut exited_sessions = Vec::new();

    for entry in &snapshot.entries {
        let Some(record) = records.get(&entry.session_id) else {
            orphaned_sessions.insert(entry.session_id);
            continue;
        };
        if instance_conflicts(record, &instance) {
            metadata.mark_lost(entry.session_id, &instance, snapshot.generation)?;
            orphaned_sessions.insert(entry.session_id);
            continue;
        }
        match entry.state {
            HolderSessionState::Running => {
                metadata.reconcile_running(entry.session_id, &instance, snapshot.generation)?
            }
            HolderSessionState::Exited => {
                let context = exit_contexts.get(&entry.session_id);
                if entry.exit_context_available && context.is_none() {
                    return Err(persist_core::PersistError::invalid_argument(
                        "exited Holder context was not queried",
                    ));
                }
                let exit_code = context
                    .map(|value| value.exit_code)
                    .or(entry.exit_code)
                    .ok_or_else(|| {
                        persist_core::PersistError::invalid_argument(
                            "exited holder session is missing exit code",
                        )
                    })?;
                let environment = context
                    .and_then(|value| value.environment.as_ref())
                    .and_then(|snapshot| {
                        persist_metadata::encode_environment(snapshot, environment_policy).ok()
                    });
                metadata.reconcile_exited_with_context(
                    entry.session_id,
                    exit_code,
                    context.and_then(|value| value.cwd.as_deref()),
                    environment.as_deref(),
                    &instance,
                    snapshot.generation,
                )?;
                exited_sessions.push(entry.session_id);
            }
        }
    }

    for record in records.values() {
        if is_active(&record.status) && !holder_ids.contains(&record.session_id) {
            metadata.mark_lost(record.session_id, &instance, snapshot.generation)?;
        }
    }
    Ok(ReconciliationResult {
        orphaned_sessions,
        exited_sessions,
    })
}

fn instance_conflicts(record: &SessionRecord, current: &str) -> bool {
    record
        .holder_instance
        .as_deref()
        .is_some_and(|instance| instance != current)
}

fn is_active(status: &str) -> bool {
    matches!(status, "running" | "attached" | "detached")
}

#[cfg(test)]
mod tests;
