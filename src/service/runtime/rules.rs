use super::model::{ServiceState, TransitionCause};

pub(super) fn is_allowed_transition(
    from: ServiceState,
    to: ServiceState,
    cause: TransitionCause,
) -> bool {
    match (from, to) {
        (ServiceState::Inactive, ServiceState::Starting) => matches!(
            cause,
            TransitionCause::ExplicitStart
                | TransitionCause::DependencyStart
                | TransitionCause::Timer
        ),
        (ServiceState::Inactive, ServiceState::Skipped) => {
            cause == TransitionCause::ConditionSkipped
        }
        (ServiceState::Inactive, ServiceState::Failed) => matches!(
            cause,
            TransitionCause::ValidationError
                | TransitionCause::AssertionError
                | TransitionCause::CycleDetected
                | TransitionCause::DependencyFailure
        ),
        (ServiceState::Starting, ServiceState::Active) => start_succeeded(cause),
        (ServiceState::Starting, ServiceState::Completed) => start_succeeded(cause),
        (ServiceState::Starting, ServiceState::Skipped) => {
            cause == TransitionCause::ConditionSkipped
        }
        (ServiceState::Starting, ServiceState::Backoff) => is_startup_retry_cause(cause),
        (ServiceState::Starting, ServiceState::Stopping) => cause == TransitionCause::ExplicitStop,
        (ServiceState::Starting, ServiceState::Failed) => matches!(
            cause,
            TransitionCause::AssertionError
                | TransitionCause::ReadinessTimeout
                | TransitionCause::PreHookFailure
                | TransitionCause::ParentSetupFailure
                | TransitionCause::PreExecFailure
                | TransitionCause::ProcessCrash
                | TransitionCause::ShutdownWave
                | TransitionCause::DependencyFailure
                | TransitionCause::ValidationError
                | TransitionCause::CycleDetected
                | TransitionCause::RestartBudgetExhausted
        ),
        (ServiceState::Active, ServiceState::Reloading) => {
            matches!(cause, TransitionCause::ExplicitReload)
        }
        (ServiceState::Active, ServiceState::Stopping) => is_stop_cause(cause),
        (ServiceState::Active, ServiceState::Backoff) => matches!(
            cause,
            TransitionCause::ProcessCrash
                | TransitionCause::CleanExitRestart
                | TransitionCause::HealthCheckFailure
                | TransitionCause::WatchdogTimeout
        ),
        (ServiceState::Active, ServiceState::Failed) => matches!(
            cause,
            TransitionCause::ProcessCrash
                | TransitionCause::HealthCheckFailure
                | TransitionCause::WatchdogTimeout
                | TransitionCause::RestartBudgetExhausted
        ),
        (ServiceState::Active, ServiceState::Inactive) => cause == TransitionCause::CleanExit,
        (ServiceState::Backoff, ServiceState::Starting) => cause == TransitionCause::RestartPolicy,
        (ServiceState::Backoff, ServiceState::Inactive) => cause == TransitionCause::ExplicitStop,
        (ServiceState::Reloading, ServiceState::Active) => {
            matches!(cause, TransitionCause::ExplicitReload)
        }
        (ServiceState::Reloading, ServiceState::Stopping) => is_stop_cause(cause),
        (ServiceState::Reloading, ServiceState::Backoff) => cause == TransitionCause::ProcessCrash,
        (ServiceState::Reloading, ServiceState::Failed) => matches!(
            cause,
            TransitionCause::ProcessCrash | TransitionCause::RestartBudgetExhausted
        ),
        (ServiceState::Stopping, ServiceState::Inactive) => matches!(
            cause,
            TransitionCause::ExplicitStop | TransitionCause::ShutdownWave
        ),
        (ServiceState::Stopping, ServiceState::Failed) => matches!(
            cause,
            TransitionCause::ConflictEviction | TransitionCause::BindsToPropagation
        ),
        (ServiceState::Stopping, ServiceState::Abandoned) => {
            cause == TransitionCause::ProcessUnkillable
        }
        (ServiceState::Stopping, ServiceState::Starting) => cause == TransitionCause::ExplicitStart,
        (ServiceState::Completed, ServiceState::Inactive) => matches!(
            cause,
            TransitionCause::CleanExit
                | TransitionCause::ExplicitStop
                | TransitionCause::ShutdownWave
        ),
        (ServiceState::Completed, ServiceState::Starting) => {
            matches!(
                cause,
                TransitionCause::ExplicitStart | TransitionCause::Timer
            )
        }
        (ServiceState::Failed, ServiceState::Starting) => matches!(
            cause,
            TransitionCause::ExplicitStart
                | TransitionCause::BindsToRecovery
                | TransitionCause::Timer
        ),
        (ServiceState::Failed, ServiceState::Inactive) => cause == TransitionCause::ExplicitReset,
        (ServiceState::Failed, ServiceState::Abandoned) => {
            cause == TransitionCause::ProcessUnkillable
        }
        (ServiceState::Abandoned, ServiceState::Inactive) => {
            cause == TransitionCause::ExplicitReset
        }
        (ServiceState::Skipped, ServiceState::Inactive) => matches!(
            cause,
            TransitionCause::ExplicitReset | TransitionCause::ExplicitStart
        ),
        _ => false,
    }
}

fn start_succeeded(cause: TransitionCause) -> bool {
    matches!(
        cause,
        TransitionCause::ExplicitStart
            | TransitionCause::DependencyStart
            | TransitionCause::RestartPolicy
            | TransitionCause::BindsToRecovery
            | TransitionCause::Timer
    )
}

fn is_startup_retry_cause(cause: TransitionCause) -> bool {
    matches!(
        cause,
        TransitionCause::ReadinessTimeout
            | TransitionCause::PreHookFailure
            | TransitionCause::ProcessCrash
            | TransitionCause::PreExecFailure
            | TransitionCause::ParentSetupFailure
    )
}

fn is_stop_cause(cause: TransitionCause) -> bool {
    matches!(
        cause,
        TransitionCause::ExplicitStop
            | TransitionCause::ConflictEviction
            | TransitionCause::BindsToPropagation
            | TransitionCause::ShutdownWave
    )
}
