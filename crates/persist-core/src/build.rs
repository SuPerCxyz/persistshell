use std::fmt;

/// Build and version metadata shown by PersistShell binaries.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VersionInfo {
    pub project: &'static str,
    pub binary: &'static str,
    pub version: &'static str,
}

impl fmt::Display for VersionInfo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.binary, self.version)
    }
}

pub fn version_info(binary: &'static str) -> VersionInfo {
    VersionInfo {
        project: "PersistShell",
        binary,
        version: env!("CARGO_PKG_VERSION"),
    }
}

pub fn version_string(binary: &'static str) -> String {
    version_info(binary).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_uses_binary_name() {
        let version = version_string("persist");
        assert!(version.starts_with("persist "));
    }
}
