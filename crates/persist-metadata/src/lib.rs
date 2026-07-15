//! Metadata storage boundary for PersistShell.

pub mod migration;
pub mod schema;
pub mod store;

pub use schema::SCHEMA_VERSION;
pub use store::{MetadataStore, SessionRecord};
