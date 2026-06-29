mod coordinator;
mod graph;
mod model;
mod planner;

pub(crate) use coordinator::run_phase2_boot_with_retained;
pub use coordinator::{
    DEFAULT_BOOT_SUCCESS_GRACE_SECS, DEFAULT_MAX_PARALLEL_STARTS, Phase2BootRun,
    Phase2BootRunError, Phase2BootSettings, Phase2RecoveryReason, run_phase2_boot,
};
pub use model::{
    BlockedReason, BlockedService, DependencyKind, Phase2BootPlan, Phase2BootPlanError,
    PreparedStart, StartCause,
};
pub use planner::prepare_phase2_boot_plan;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod coordinator_tests;
