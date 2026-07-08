#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct WindowSize {
    pub rows: u16,
    pub cols: u16,
}

impl WindowSize {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self { rows, cols }
    }
}
