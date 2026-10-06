//! The kebab-case values peinit's enumerated event fields take (PGSS §6.5).
//!
//! The same vocabularies the control channel spells in snake_case (PSPU
//! §4.B); an event writes each word with hyphens, as every event field's
//! value is written.

use crate::fd_store::StoreFdOutcome;
use crate::job::{JobState, JobType};
use crate::operation::{OperationSource, OperationState, OperationType};
use crate::service::ServiceDependencyKind;
use crate::service::runtime::{ServiceState, TransitionCause};

pub(super) fn job_type_label(value: JobType) -> &'static str {
    match value {
        JobType::ServiceMain => "service-main",
        JobType::PreExecHook => "pre-exec-hook",
        JobType::PostExecHook => "post-exec-hook",
        JobType::ReloadHook => "reload-hook",
        JobType::HealthCheck => "health-check",
        JobType::Submitted => "submitted",
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
        OperationSource::DependencyPropagation => "dependency-propagation",
        OperationSource::RestartPolicy => "restart-policy",
        OperationSource::Timer => "timer",
        OperationSource::BindsToRecovery => "binds-to-recovery",
        OperationSource::BindsToPropagation => "binds-to-propagation",
        OperationSource::ConflictResolution => "conflict-resolution",
        OperationSource::OnFailure => "on-failure",
        OperationSource::TtyRelease => "tty-release",
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
        ServiceDependencyKind::BindsTo => "binds-to",
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
        TransitionCause::ExplicitStart => "explicit-start",
        TransitionCause::DependencyStart => "dependency-start",
        TransitionCause::RestartPolicy => "restart-policy",
        TransitionCause::BindsToRecovery => "binds-to-recovery",
        TransitionCause::ExplicitStop => "explicit-stop",
        TransitionCause::ExplicitReload => "explicit-reload",
        TransitionCause::ExplicitReset => "explicit-reset",
        TransitionCause::ConflictEviction => "conflict-eviction",
        TransitionCause::BindsToPropagation => "binds-to-propagation",
        TransitionCause::Timer => "timer",
        TransitionCause::ShutdownWave => "shutdown-wave",
        TransitionCause::ProcessCrash => "process-crash",
        TransitionCause::CleanExit => "clean-exit",
        TransitionCause::CleanExitRestart => "clean-exit-restart",
        TransitionCause::ReadinessTimeout => "readiness-timeout",
        TransitionCause::WatchdogTimeout => "watchdog-timeout",
        TransitionCause::HealthCheckFailure => "health-check-failure",
        TransitionCause::PreHookFailure => "pre-hook-failure",
        TransitionCause::ParentSetupFailure => "parent-setup-failure",
        TransitionCause::PreExecFailure => "pre-exec-failure",
        TransitionCause::DependencyFailure => "dependency-failure",
        TransitionCause::RestartBudgetExhausted => "restart-budget-exhausted",
        TransitionCause::CycleDetected => "cycle-detected",
        TransitionCause::ValidationError => "validation-error",
        TransitionCause::AssertionError => "assertion-error",
        TransitionCause::ConditionSkipped => "condition-skipped",
        TransitionCause::TtyUnavailable => "tty-unavailable",
        TransitionCause::ProcessUnkillable => "process-unkillable",
        TransitionCause::InternalError => "internal-error",
    }
}

pub(super) fn service_type_label(value: crate::service::ServiceType) -> &'static str {
    match value {
        crate::service::ServiceType::Simple => "simple",
        crate::service::ServiceType::Oneshot => "oneshot",
    }
}
