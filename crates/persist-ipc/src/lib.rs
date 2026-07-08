//! Unix socket protocol boundary for PersistShell.

pub mod protocol;
pub mod socket;

pub use protocol::{ProtocolVersion, RequestKind};
