pub mod phase2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootMode {
    Full,
    Safe,
    Recovery,
}
