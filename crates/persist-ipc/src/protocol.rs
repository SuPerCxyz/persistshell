#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    pub const CURRENT: Self = Self { major: 0, minor: 1 };
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RequestKind {
    NewSession,
    AttachSession,
    DetachSession,
    ListSessions,
    KillSession,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_protocol_starts_at_zero_one() {
        assert_eq!(ProtocolVersion::CURRENT.major, 0);
        assert_eq!(ProtocolVersion::CURRENT.minor, 1);
    }
}
