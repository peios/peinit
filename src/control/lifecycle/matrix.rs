use crate::service::runtime::{ServiceState, TransitionCause};

use super::model::LifecycleCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandAdmission {
    Operation {
        expectation: OperationExpectation,
    },
    SynchronousClear {
        cause: TransitionCause,
        result: &'static str,
    },
    Already,
    Noop,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OperationExpectation {
    Any,
    DeferredStart,
    Merge,
    Queue,
}

/// Whether a start request against a service already in `state` is a no-op.
///
/// This is the admission matrix's `Already` verdict for `Start`, exposed so the
/// relationship-start paths (`OnFailure`, `BindsTo` recovery) can answer the
/// same question the control interface does without going through admission.
/// They must: a service in one of these states has no legal transition into
/// `Starting`, so planning a start for it ends in `InvalidTransition` out of a
/// failure-reaction path rather than a benign refusal (PEI-597).
pub fn start_is_already_satisfied(state: ServiceState) -> bool {
    matches!(classify_start(state), CommandAdmission::Already)
}

pub(super) fn classify(command: LifecycleCommand, state: ServiceState) -> CommandAdmission {
    match command {
        LifecycleCommand::Start => classify_start(state),
        LifecycleCommand::Stop => classify_stop(state),
        LifecycleCommand::Restart => classify_restart(state),
        LifecycleCommand::Reload => classify_reload(state),
        LifecycleCommand::Reset => classify_reset(state),
    }
}

fn classify_start(state: ServiceState) -> CommandAdmission {
    match state {
        ServiceState::Inactive
        | ServiceState::Completed
        | ServiceState::Failed
        | ServiceState::Skipped => CommandAdmission::Operation {
            expectation: OperationExpectation::Any,
        },
        ServiceState::Starting => CommandAdmission::Operation {
            expectation: OperationExpectation::Merge,
        },
        ServiceState::Backoff => CommandAdmission::Operation {
            expectation: OperationExpectation::DeferredStart,
        },
        ServiceState::Active | ServiceState::Reloading => CommandAdmission::Already,
        ServiceState::Stopping => CommandAdmission::Operation {
            expectation: OperationExpectation::Queue,
        },
        ServiceState::Abandoned => CommandAdmission::Invalid,
    }
}

fn classify_stop(state: ServiceState) -> CommandAdmission {
    match state {
        ServiceState::Inactive | ServiceState::Failed | ServiceState::Skipped => {
            CommandAdmission::Noop
        }
        ServiceState::Starting | ServiceState::Active | ServiceState::Reloading => {
            CommandAdmission::Operation {
                expectation: OperationExpectation::Any,
            }
        }
        ServiceState::Stopping => CommandAdmission::Operation {
            expectation: OperationExpectation::Merge,
        },
        ServiceState::Completed => CommandAdmission::SynchronousClear {
            cause: TransitionCause::ExplicitStop,
            result: "inactive",
        },
        ServiceState::Backoff => CommandAdmission::SynchronousClear {
            cause: TransitionCause::ExplicitStop,
            result: "inactive",
        },
        ServiceState::Abandoned => CommandAdmission::Invalid,
    }
}

fn classify_restart(state: ServiceState) -> CommandAdmission {
    match state {
        ServiceState::Inactive
        | ServiceState::Active
        | ServiceState::Reloading
        | ServiceState::Completed
        | ServiceState::Backoff
        | ServiceState::Failed
        | ServiceState::Skipped => CommandAdmission::Operation {
            expectation: OperationExpectation::Any,
        },
        ServiceState::Starting | ServiceState::Stopping => CommandAdmission::Operation {
            expectation: OperationExpectation::Queue,
        },
        ServiceState::Abandoned => CommandAdmission::Invalid,
    }
}

fn classify_reload(state: ServiceState) -> CommandAdmission {
    match state {
        ServiceState::Active => CommandAdmission::Operation {
            expectation: OperationExpectation::Any,
        },
        ServiceState::Reloading => CommandAdmission::Operation {
            expectation: OperationExpectation::Merge,
        },
        ServiceState::Inactive
        | ServiceState::Starting
        | ServiceState::Stopping
        | ServiceState::Completed
        | ServiceState::Backoff
        | ServiceState::Failed
        | ServiceState::Abandoned
        | ServiceState::Skipped => CommandAdmission::Invalid,
    }
}

fn classify_reset(state: ServiceState) -> CommandAdmission {
    match state {
        ServiceState::Inactive => CommandAdmission::Noop,
        ServiceState::Failed | ServiceState::Abandoned | ServiceState::Skipped => {
            CommandAdmission::SynchronousClear {
                cause: TransitionCause::ExplicitReset,
                result: "inactive",
            }
        }
        ServiceState::Starting
        | ServiceState::Active
        | ServiceState::Reloading
        | ServiceState::Stopping
        | ServiceState::Completed
        | ServiceState::Backoff => CommandAdmission::Invalid,
    }
}
