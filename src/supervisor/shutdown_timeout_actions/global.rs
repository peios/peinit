use crate::shutdown::ShutdownFinalizationState;

use crate::supervisor::work::SupervisorWork;

pub(in crate::supervisor) fn shutdown_global_timeout_due(
    work: &SupervisorWork,
    now_ns: u64,
) -> bool {
    work.shutdown.as_ref().is_some_and(|shutdown| {
        matches!(
            shutdown.finalization,
            ShutdownFinalizationState::WaitingForServices
        ) && now_ns >= shutdown.global_deadline_ns
    })
}
