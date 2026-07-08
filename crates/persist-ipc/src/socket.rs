use std::path::PathBuf;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SocketEndpoint {
    pub path: PathBuf,
}

impl SocketEndpoint {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}
