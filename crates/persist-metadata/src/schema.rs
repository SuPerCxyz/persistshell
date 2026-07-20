pub const SCHEMA_VERSION: u32 = 7;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_version_matches_latest_migration() {
        assert_eq!(SCHEMA_VERSION, 7);
    }
}
