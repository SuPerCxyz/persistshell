#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
}

pub const INITIAL_MIGRATION: Migration = Migration {
    version: 1,
    name: "initial metadata schema",
};
