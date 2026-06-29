use crate::execution::job_terminal::ServiceMainJobTerminalDispatch;
use crate::execution::start::StartExecutionDispatch;
use crate::service::ServiceType;
use crate::service::runtime::ServiceState;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::start::dispatch_timer_start_for_work;

pub(in crate::supervisor) fn start_pending_timer_runs_after_terminal(
    work: &mut SupervisorWork,
    terminal: &ServiceMainJobTerminalDispatch,
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    let mut start_dispatches = Vec::new();
    for transition in &terminal.service_transitions {
        if !matches!(
            transition.event.to,
            ServiceState::Completed | ServiceState::Inactive
        ) {
            continue;
        }
        let service = transition.event.service.as_str();
        let Some(definition) = work.services.definition(service) else {
            continue;
        };
        if definition.service_type != ServiceType::Oneshot {
            continue;
        }
        let pending = work
            .services
            .runtime(service)
            .is_some_and(|runtime| runtime.pending_timer);
        if !pending {
            continue;
        }
        work.services
            .clear_pending_timer(service)
            .map_err(|source| {
                SupervisorError::Lifecycle(
                    crate::control::lifecycle::LifecycleCommandError::ServiceTable(source),
                )
            })?;
        let timer_start =
            dispatch_timer_start_for_work(work, service, observed_at_ns, max_parallel_starts)?;
        start_dispatches.extend(timer_start.start_dispatches);
    }
    Ok(start_dispatches)
}
