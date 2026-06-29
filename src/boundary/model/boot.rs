use super::error::BoundaryError;

pub trait BootAttemptCounter {
    fn reset_boot_attempt_counter(&mut self) -> Result<(), BoundaryError>;
}
