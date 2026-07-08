//! Metadata storage boundary for PersistShell.
//!
//! M01 defines the API shape without selecting a concrete SQLite crate.

pub mod migration;
pub mod schema;

pub use schema::SCHEMA_VERSION;
