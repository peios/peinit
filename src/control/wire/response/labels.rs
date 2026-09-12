use crate::job::JobType;
use crate::operation::{OperationSource, OperationState, OperationType};
use crate::service::runtime::{ServiceHealthStatus, ServiceState, TransitionCause};

pub(super) fn service_state_wire(state: ServiceState) -> &'static str {
    match state {
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

pub(super) fn service_health_wire(status: ServiceHealthStatus) -> &'static str {
    match status {
        ServiceHealthStatus::Unknown => "unknown",
        ServiceHealthStatus::Healthy => "healthy",
        ServiceHealthStatus::Unhealthy => "unhealthy",
    }
}

pub(super) fn transition_cause_wire(cause: TransitionCause) -> &'static str {
    match cause {
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
        TransitionCause::TtyUnavailable => "tty_unavailable",
        TransitionCause::ProcessUnkillable => "process_unkillable",
        TransitionCause::InternalError => "internal_error",
    }
}

pub(super) fn job_type_wire(job_type: JobType) -> &'static str {
    match job_type {
        JobType::ServiceMain => "service_main",
        JobType::PreExecHook => "pre_exec_hook",
        JobType::PostExecHook => "post_exec_hook",
        JobType::ReloadHook => "reload_hook",
        JobType::HealthCheck => "health_check",
        JobType::Submitted => "submitted",
    }
}

pub(super) fn operation_type_wire(operation_type: OperationType) -> &'static str {
    match operation_type {
        OperationType::Start => "start",
        OperationType::Stop => "stop",
        OperationType::Restart => "restart",
        OperationType::Reload => "reload",
        OperationType::Reset => "reset",
    }
}

pub(super) fn operation_source_wire(source: OperationSource) -> &'static str {
    match source {
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
        OperationSource::TtyRelease => "tty_release",
    }
}

pub(super) fn operation_state_wire(state: OperationState) -> &'static str {
    match state {
        OperationState::Pending => "pending",
        OperationState::Running => "running",
        OperationState::Completed => "completed",
        OperationState::Failed => "failed",
        OperationState::Merged => "merged",
        OperationState::Cancelled => "cancelled",
        OperationState::Aborted => "aborted",
    }
}
