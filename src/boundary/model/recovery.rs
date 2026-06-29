use super::error::BoundaryError;

pub trait RecoveryConsole {
    /// Enters the recovery shell loop. Returning `Ok(())` means recovery was
    /// intentionally left by a higher-level test harness; production
    /// implementations should only return `Err` if recovery cannot be delivered.
    fn run_recovery_forever(&mut self, reason: &str) -> Result<(), BoundaryError>;
}
