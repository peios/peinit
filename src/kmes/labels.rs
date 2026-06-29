use crate::fd_store::StoreFdOutcome;
use crate::job::{JobState, JobType};
use crate::operation::{OperationSource, OperationState, OperationType};
use crate::service::ServiceDependencyKind;
use crate::service::runtime::{ServiceState, TransitionCause};

pub(super) fn job_type_label(value: JobType) -> &'static str {
    match value {
        JobType::ServiceMain => "service_main",
        JobType::PreExecHook => "pre_exec_hook",
        JobType::PostExecHook => "post_exec_hook",
        JobType::ReloadHook => "reload_hook",
        JobType::HealthCheck => "health_check",
        JobType::AdHoc => "ad_hoc",
    }
}

pub(super) fn job_state_label(value: JobState) -> &'static str {
    match value {
        JobState::Created => "created",
        JobState::Running => "running",
        JobState::Completed => "completed",
        JobState::Failed => "failed",
        JobState::Abandoned => "abandoned",
    }
}

pub(super) fn operation_type_label(value: OperationType) -> &'static str {
    match value {
        OperationType::Start => "start",
        OperationType::Stop => "stop",
        OperationType::Restart => "restart",
        OperationType::Reload => "reload",
        OperationType::Reset => "reset",
    }
}

pub(super) fn operation_source_label(value: OperationSource) -> &'static str {
    match value {
        OperationSource::Admin => "admin",
        OperationSource::Boot => "boot",
        OperationSource::Shutdown => "shutdown",
        OperationSource::DependencyPropagation => "dependency_propagation",
        OperationSource::RestartPolicy => "restart_policy",
        OperationSource::Timer => "timer",
        OperationSource::BindsToRecovery => "binds_to_recovery",
        OperationSource::BindsToPropagation => "binds_to_propagation",
        OperationSource::ConflictResolution => "conflict_resolution",
        OperationSource::OnFailure => "on_failure",
    }
}

pub(super) fn operation_state_label(value: OperationState) -> &'static str {
    match value {
        OperationState::Pending => "pending",
        OperationState::Running => "running",
        OperationState::Completed => "completed",
        OperationState::Failed => "failed",
        OperationState::Merged => "merged",
        OperationState::Cancelled => "cancelled",
        OperationState::Aborted => "aborted",
    }
}

pub(super) fn fd_store_outcome_label(value: StoreFdOutcome) -> &'static str {
    match value {
        StoreFdOutcome::Stored => "stored",
        StoreFdOutcome::Disabled => "disabled",
        StoreFdOutcome::Full => "full",
    }
}

pub(super) fn service_dependency_kind_label(value: ServiceDependencyKind) -> &'static str {
    match value {
        ServiceDependencyKind::Requires => "requires",
        ServiceDependencyKind::Wants => "wants",
        ServiceDependencyKind::BindsTo => "binds_to",
    }
}

pub(super) fn service_state_label(value: ServiceState) -> &'static str {
    match value {
        ServiceState::Inactive => "inactive",
        ServiceState::Starting => "starting",
        ServiceState::Active => "active",
        ServiceState::Reloading => "reloading",
        ServiceState::Stopping => "stopping",
        ServiceState::Completed => "completed",
        ServiceState::Backoff => "backoff",
        ServiceState::Failed => "failed",
        ServiceState::Abandoned => "abandoned",
        ServiceState::Skipped => "skipped",
    }
}

pub(super) fn transition_cause_label(value: TransitionCause) -> &'static str {
    match value {
        TransitionCause::ExplicitStart => "explicit_start",
        TransitionCause::DependencyStart => "dependency_start",
        TransitionCause::RestartPolicy => "restart_policy",
        TransitionCause::BindsToRecovery => "binds_to_recovery",
        TransitionCause::ExplicitStop => "explicit_stop",
        TransitionCause::ExplicitReload => "explicit_reload",
        TransitionCause::ExplicitReset => "explicit_reset",
        TransitionCause::ConflictEviction => "conflict_eviction",
        TransitionCause::BindsToPropagation => "binds_to_propagation",
        TransitionCause::Timer => "timer",
        TransitionCause::ShutdownWave => "shutdown_wave",
        TransitionCause::ProcessCrash => "process_crash",
        TransitionCause::CleanExit => "clean_exit",
        TransitionCause::CleanExitRestart => "clean_exit_restart",
        TransitionCause::ReadinessTimeout => "readiness_timeout",
        TransitionCause::WatchdogTimeout => "watchdog_timeout",
        TransitionCause::HealthCheckFailure => "health_check_failure",
        TransitionCause::PreHookFailure => "pre_hook_failure",
        TransitionCause::ParentSetupFailure => "parent_setup_failure",
        TransitionCause::PreExecFailure => "pre_exec_failure",
        TransitionCause::DependencyFailure => "dependency_failure",
        TransitionCause::RestartBudgetExhausted => "restart_budget_exhausted",
        TransitionCause::CycleDetected => "cycle_detected",
        TransitionCause::ValidationError => "validation_error",
        TransitionCause::AssertionError => "assertion_error",
        TransitionCause::ConditionSkipped => "condition_skipped",
        TransitionCause::ProcessUnkillable => "process_unkillable",
    }
}
