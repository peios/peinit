mod linux_finalizer;
mod model;
mod mountinfo;
mod plan;

#[cfg(test)]
mod tests;

pub use linux_finalizer::LinuxShutdownFinalizer;
pub use model::{
    CleanupActionResult, MountCleanupResult, ShutdownDeadline, ShutdownDeadlineKind, ShutdownError,
    ShutdownFinalizationReport, ShutdownFinalizationState, ShutdownIgnoredService, ShutdownKind,
    ShutdownPlan, ShutdownPlanError, ShutdownPostKillDeadline, ShutdownRuntime, ShutdownSettings,
    ShutdownSignal, ShutdownSignalTracker, ShutdownStopDeadline, ShutdownStopParticipant,
    ShutdownStopWave,
};
pub use plan::plan_graceful_shutdown;
