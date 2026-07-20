mod client;
mod data;
mod process;
mod process_watch;
mod reconcile;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use persist_core::shell_state::EnvironmentSnapshot;
use persist_core::{PersistError, Result};
use persist_ipc::holder::{CreateSessionRequest, HolderAttachMode, HolderSessionEntry};

use client::{ControlInventorySnapshot, HolderControlClient};
pub(crate) use data::HolderDataConnection;
pub(crate) use reconcile::reconcile_metadata;

pub(crate) struct HolderRuntime {
    client: HolderControlClient,
    socket_path: std::path::PathBuf,
    replay_bytes: u32,
    cache: Arc<RwLock<HolderInventorySnapshot>>,
    process_exit: process_watch::ProcessExit,
    connected: AtomicBool,
    child: std::sync::Mutex<Option<std::process::Child>>,
    operations: std::sync::Mutex<()>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExitContext {
    pub(crate) session_id: u32,
    pub(crate) exit_code: i32,
    pub(crate) cwd: Option<String>,
    pub(crate) environment: Option<EnvironmentSnapshot>,
}

#[derive(Clone)]
pub(crate) struct HolderInventorySnapshot {
    pub(crate) instance_id: [u8; 16],
    pub(crate) generation: u64,
    pub(crate) entries: Vec<HolderSessionEntry>,
}

impl From<ControlInventorySnapshot> for HolderInventorySnapshot {
    fn from(snapshot: ControlInventorySnapshot) -> Self {
        Self {
            instance_id: snapshot.instance_id,
            generation: snapshot.generation,
            entries: snapshot.entries,
        }
    }
}

impl HolderInventorySnapshot {
    pub(crate) fn instance_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(32);
        for byte in self.instance_id {
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
        encoded
    }
}

impl HolderRuntime {
    pub(crate) fn initialize(
        runtime_dir: &Path,
        socket_path: &Path,
        replay_bytes: u32,
    ) -> Result<Option<Self>> {
        let Some(binary) = process::resolve_holder_binary()? else {
            return Ok(None);
        };
        let (client, child) = process::connect_or_start(runtime_dir, socket_path, &binary)?;
        let process_exit = process_watch::ProcessExit::watch(client.holder_pid())?;
        let cache = Arc::new(RwLock::new(client.inventory_snapshot()?.into()));
        Ok(Some(Self {
            client,
            socket_path: socket_path.to_path_buf(),
            replay_bytes,
            cache,
            process_exit,
            connected: AtomicBool::new(true),
            child: std::sync::Mutex::new(child),
            operations: std::sync::Mutex::new(()),
        }))
    }

    pub(crate) fn inventory_snapshot(&self) -> Vec<HolderSessionEntry> {
        self.cache.read().unwrap().entries.clone()
    }

    pub(crate) fn reconciliation_snapshot(&self) -> HolderInventorySnapshot {
        self.cache.read().unwrap().clone()
    }

    pub(crate) fn exit_contexts(
        &self,
        snapshot: &HolderInventorySnapshot,
    ) -> Result<HashMap<u32, ExitContext>> {
        snapshot
            .entries
            .iter()
            .filter(|entry| entry.exit_context_available)
            .map(|entry| {
                self.exit_context(entry.session_id)
                    .map(|context| (entry.session_id, context))
            })
            .collect()
    }

    pub(crate) fn has_exited(&self) -> Result<bool> {
        self.process_exit.has_exited()
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire) && self.has_exited().is_ok_and(|exited| !exited)
    }

    pub(crate) fn pid(&self) -> u32 {
        self.client.holder_pid()
    }

    pub(crate) fn instance_hex(&self) -> String {
        self.cache.read().unwrap().instance_hex()
    }

    pub(crate) fn mark_unavailable(&self) -> HolderInventorySnapshot {
        self.connected.store(false, Ordering::Release);
        let mut cache = self.cache.write().unwrap();
        cache.entries.clear();
        cache.clone()
    }

    pub(crate) fn refresh_inventory(&self) -> Result<()> {
        let _operation = self.operations.lock().unwrap();
        self.refresh_inventory_locked()
    }

    fn refresh_inventory_locked(&self) -> Result<()> {
        let result = match self.client.inventory_snapshot() {
            Ok(inventory) => inventory,
            Err(PersistError::Io { .. }) => {
                if let Err(error) = self.client.reconnect() {
                    self.connected.store(false, Ordering::Release);
                    return Err(error);
                }
                match self.client.inventory_snapshot() {
                    Ok(inventory) => inventory,
                    Err(error) => {
                        self.connected.store(false, Ordering::Release);
                        return Err(error);
                    }
                }
            }
            Err(error) => {
                self.connected.store(false, Ordering::Release);
                return Err(error);
            }
        };
        *self.cache.write().unwrap() = result.into();
        self.connected.store(true, Ordering::Release);
        Ok(())
    }

    pub(crate) fn create(&self, request: CreateSessionRequest) -> Result<()> {
        let _operation = self.operations.lock().unwrap();
        self.client.create(request)?;
        self.refresh_inventory_locked()
    }

    pub(crate) fn exit_context(&self, session_id: u32) -> Result<ExitContext> {
        let _operation = self.operations.lock().unwrap();
        self.client.exit_context(session_id)
    }

    pub(crate) fn close(&self, session_id: u32) -> Result<ExitContext> {
        let _operation = self.operations.lock().unwrap();
        self.client.close(session_id)?;
        self.client.wait_for_session_exit(session_id)
    }

    pub(crate) fn retire_exited(&self, session_id: u32) -> Result<()> {
        let _operation = self.operations.lock().unwrap();
        self.client.retire_exited(session_id)?;
        self.refresh_inventory_locked()
    }

    pub(crate) fn kill(&self, session_id: u32) -> Result<()> {
        let _operation = self.operations.lock().unwrap();
        self.client.kill(session_id)
    }

    pub(crate) fn attach(
        &self,
        session_id: u32,
        mode: HolderAttachMode,
    ) -> Result<HolderDataConnection> {
        let _operation = self.operations.lock().unwrap();
        let connection = data::HolderDataConnection::connect(
            &self.socket_path,
            self.client.instance_id(),
            self.client.nonce(),
            self.client.protocol_minor(),
            session_id,
            mode,
            self.replay_bytes,
        )?;
        self.refresh_inventory_locked()?;
        Ok(connection)
    }

    pub(crate) fn shutdown(&self) -> Result<()> {
        if !self.process_exit.has_exited()? {
            self.client.shutdown_all()?;
            self.process_exit.wait()?;
        }
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            child.wait().map_err(|source| PersistError::Io {
                operation: "reap persist-holder",
                source,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
