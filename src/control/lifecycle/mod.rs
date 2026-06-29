mod admission;
mod dependency_admission;
mod matrix;
mod model;
mod start_plan;
mod synchronous_clear;

#[cfg(test)]
mod tests;

pub use admission::admit_lifecycle_command;
pub use dependency_admission::admit_lifecycle_command_with_operation_ids;
pub use model::{
    LifecycleCommand, LifecycleCommandError, LifecycleCommandOutcome, LifecycleCommandRequest,
    ServiceStatusSnapshot, SynchronousClearOutcome,
};
pub use start_plan::{
    DependencyAvailability, OnDemandStartDispatch, OnDemandStartDispatchError, OnDemandStartPlan,
    OnDemandStartPlanError, PlannedStart, StartBlockReason, StartPlanBlockedService,
    dispatch_existing_requested_start_plan, dispatch_on_demand_start_plan,
    plan_binds_to_recovery_start, plan_on_demand_start, plan_on_failure_start,
    plan_restart_policy_start, plan_timer_start,
};
