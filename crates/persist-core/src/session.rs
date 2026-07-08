use std::fmt;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SessionStatus {
    Creating,
    Running,
    Detached,
    Closed,
    Killed,
    Zombie,
    Recovering,
    Archived,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = match self {
            Self::Creating => "creating",
            Self::Running => "running",
            Self::Detached => "detached",
            Self::Closed => "closed",
            Self::Killed => "killed",
            Self::Zombie => "zombie",
            Self::Recovering => "recovering",
            Self::Archived => "archived",
        };
        formatter.write_str(status)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum AttachMode {
    ReadWrite,
    ReadOnly,
    Takeover,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_status_matches_documented_name() {
        assert_eq!(SessionStatus::Closed.to_string(), "closed");
    }
}
