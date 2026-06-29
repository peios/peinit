use crate::boundary::{BoundaryError, ProcessLaunchError};
use crate::execution::launch::LaunchCreatedJobError;
use crate::service::runtime::TransitionCause;

pub(super) fn classify_launch_failure(
    error: &LaunchCreatedJobError,
) -> Option<ClassifiedLaunchFailure> {
    let LaunchCreatedJobError::Boundary(error) = error else {
        return None;
    };
    match error {
        BoundaryError::Token(message) => Some(ClassifiedLaunchFailure::parent_setup(format!(
            "token materialization failed: {message}"
        ))),
        BoundaryError::Process(message) => {
            Some(ClassifiedLaunchFailure::parent_setup(message.clone()))
        }
        BoundaryError::ProcessLaunch(ProcessLaunchError::ParentSetup { message, .. }) => {
            Some(ClassifiedLaunchFailure::parent_setup(message.clone()))
        }
        BoundaryError::ProcessLaunch(ProcessLaunchError::PreExec { error, .. }) => {
            Some(ClassifiedLaunchFailure {
                cause: TransitionCause::PreExecFailure,
                reason: format!(
                    "PreExecFailure: {} failed with errno {}",
                    error.step.label(),
                    error.errno
                ),
            })
        }
        BoundaryError::ProcessLaunch(ProcessLaunchError::MalformedPreExec { message, .. }) => {
            Some(ClassifiedLaunchFailure {
                cause: TransitionCause::PreExecFailure,
                reason: format!("PreExecFailure: malformed child setup evidence: {message}"),
            })
        }
        _ => None,
    }
}

pub(super) fn pre_start_hook_launch_failure_reason(
    error: &LaunchCreatedJobError,
) -> Option<String> {
    let LaunchCreatedJobError::Boundary(error) = error else {
        return None;
    };
    let detail = match error {
        BoundaryError::Token(message) => format!("token materialization failed: {message}"),
        BoundaryError::Process(message) => message.clone(),
        BoundaryError::ProcessLaunch(ProcessLaunchError::ParentSetup { message, .. }) => {
            message.clone()
        }
        BoundaryError::ProcessLaunch(ProcessLaunchError::PreExec { error, .. }) => {
            format!("{} failed with errno {}", error.step.label(), error.errno)
        }
        BoundaryError::ProcessLaunch(ProcessLaunchError::MalformedPreExec { message, .. }) => {
            format!("malformed child setup evidence: {message}")
        }
        _ => return None,
    };
    Some(format!("PreHookFailure: launch failed: {detail}"))
}

pub(super) struct ClassifiedLaunchFailure {
    pub cause: TransitionCause,
    pub reason: String,
}

impl ClassifiedLaunchFailure {
    fn parent_setup(message: String) -> Self {
        Self {
            cause: TransitionCause::ParentSetupFailure,
            reason: format!("ParentSetupFailure: {message}"),
        }
    }
}
