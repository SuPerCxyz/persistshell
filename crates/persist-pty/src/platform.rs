#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Platform {
    Linux,
}

pub fn current_platform() -> Platform {
    Platform::Linux
}
