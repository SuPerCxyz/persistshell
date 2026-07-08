#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SessionSignal {
    Interrupt,
    Quit,
    Suspend,
    WindowChanged,
}
