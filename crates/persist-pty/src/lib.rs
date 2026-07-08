//! PTY boundary for PersistShell.
//!
//! M01 only defines the crate and module boundary. Real PTY ownership and
//! Linux syscall integration start in the PTY Engine milestone.

pub mod platform;
pub mod process;
pub mod signal;
pub mod termios;

use persist_core::{PersistError, Result};

#[derive(Debug, Default)]
pub struct PtyEngine;

impl PtyEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn open_session(&self) -> Result<()> {
        Err(PersistError::not_implemented("PTY Engine MVP"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_engine_reports_not_implemented() {
        let error = PtyEngine::new()
            .open_session()
            .expect_err("pty is not implemented yet");
        assert!(error.to_string().contains("PTY Engine MVP"));
    }
}
