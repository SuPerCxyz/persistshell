//! Metadata storage boundary for PersistShell.

pub mod environment;
pub mod migration;
pub mod schema;
pub mod store;

pub use environment::{decode_environment, encode_environment};
pub use schema::SCHEMA_VERSION;
pub use store::{MetadataStore, SessionRecord};
