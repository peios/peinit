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

#[cfg(test)]
mod tests {
    use super::classify_launch_failure;
    use crate::boundary::{BoundaryError, ProcessLaunchError};
    use crate::execution::launch::LaunchCreatedJobError;
    use crate::service::runtime::TransitionCause;

    /// TRM §5.3 — malformed evidence fails closed. When the parent reads a
    /// setup-pipe payload that is not exactly eight bytes, carries an unknown
    /// step id, or an errno of zero or less, the child's report is unusable, and
    /// peinit must still fail the start rather than treat the exec as having
    /// succeeded. The failure classifies as `PreExecFailure`, the same
    /// fail-closed direction a decoded child failure takes.
    ///
    /// No guest can reach this: the payload is written by peinit's own
    /// pre-exec child over an internal pipe, so a malformed one cannot be
    /// injected from userspace.
    #[test]
    fn malformed_child_setup_evidence_fails_closed_as_pre_exec_failure() {
        let error = LaunchCreatedJobError::Boundary(BoundaryError::ProcessLaunch(
            ProcessLaunchError::malformed_pre_exec("expected 8 bytes, got 3", Vec::new()),
        ));

        let classified = classify_launch_failure(&error).expect("classified");

        assert_eq!(classified.cause, TransitionCause::PreExecFailure);
        assert!(
            classified.reason.starts_with("PreExecFailure:"),
            "fails closed as a PreExecFailure: {}",
            classified.reason,
        );
        assert!(
            classified.reason.contains("malformed child setup evidence"),
            "and names the malformed evidence: {}",
            classified.reason,
        );
    }
}
