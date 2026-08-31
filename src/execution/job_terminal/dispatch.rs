use crate::job::JobEvent;
use crate::service::ServiceType;
use crate::service::runtime::ServiceState;

use super::super::start::StartReadyContext;
use super::active::{apply_active_simple_exit, apply_reloading_simple_exit};
use super::ended::validate_terminal_service_main_event;
use super::model::{ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError};
use super::start::{apply_oneshot_start_success, apply_start_process_failure};
use super::stopping::apply_stopping_simple_exit;

/// Apply a service-main job's terminal event to the service that owns it.
///
/// The state machine has four states in which a main process is still the
/// service's own: `Starting`, and `Active`/`Reloading`/`Stopping` for a simple
/// service. Everything else means the exit is *late* — the service already
/// reached a state that had stopped expecting a process, and the exit is
/// telling us about one the state machine has already accounted for.
///
/// A late exit is normal, not an invariant violation, and PID 1 must survive
/// it. Two supported paths produce one:
///
///   - `Abandoned`. A stop whose cgroup is still populated after SIGKILL gives
///     up and marks the service abandoned with a leaked cgroup, deliberately
///     leaving the main job open because the process is still there. When it
///     finally dies — the uninterruptible sleep it was stuck in completes — its
///     exit arrives here.
///   - `Backoff` and the other no-process states. A service that reached them
///     by a route other than its own main exit (a health-check escalation, a
///     watchdog timeout, a failure propagated across the start graph) can still
///     have the process outlive the transition by a turn.
///
/// Treating those as `UnsupportedServiceState` — a runtime-loop failure, which
/// takes PID 1 into recovery — turned one flapping service into a dead machine
/// (PEI-531). The cost of a genuine inconsistency reaching here is a service
/// left in a stale state and one console line; the cost of the old behaviour
/// was the machine, so the trade is not close.
pub fn apply_service_main_job_terminal(
    context: &mut StartReadyContext<'_>,
    job_event: JobEvent,
) -> Result<ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError> {
    let terminal = validate_terminal_service_main_event(&job_event)?;
    let service = terminal.service;
    let ended = terminal.ended;
    let definition = context
        .services
        .definition(&service)
        .ok_or_else(|| {
            ServiceMainJobTerminalError::ServiceTable(
                crate::service::ServiceTableError::UnknownService {
                    service: service.clone(),
                },
            )
        })?
        .clone();
    let state = context
        .services
        .runtime(&service)
        .ok_or_else(|| {
            ServiceMainJobTerminalError::ServiceTable(
                crate::service::ServiceTableError::UnknownService {
                    service: service.clone(),
                },
            )
        })?
        .state;

    let succeeded = ended.is_success_for(&definition);
    match (state, definition.service_type, succeeded) {
        (ServiceState::Starting, ServiceType::Oneshot, true) => {
            apply_oneshot_start_success(context, &service, job_event, ended)
        }
        (ServiceState::Starting, _, _) => apply_start_process_failure(
            context.services,
            context.operations,
            context.graph,
            &service,
            job_event,
            ended,
        ),
        (ServiceState::Active, ServiceType::Simple, _) => {
            apply_active_simple_exit(context.services, &service, job_event, &definition, ended)
        }
        (ServiceState::Reloading, ServiceType::Simple, _) => {
            apply_reloading_simple_exit(context.services, &service, job_event, &definition, ended)
        }
        (ServiceState::Stopping, ServiceType::Simple, _) => apply_stopping_simple_exit(
            context.services,
            context.operations,
            &service,
            job_event,
            ended,
        ),
        _ => Ok(late_exit(job_event, state)),
    }
}

/// Record a late exit without acting on it.
///
/// Nothing to undo and nothing to drive: the job record was consumed producing
/// this event, and the service's state was reached by whatever path already
/// accounted for the process. The empty dispatch says exactly that, and
/// `late_exit` is what the console reports so the inconsistency is visible
/// rather than silent.
fn late_exit(job_event: JobEvent, state: ServiceState) -> ServiceMainJobTerminalDispatch {
    ServiceMainJobTerminalDispatch {
        job_event,
        operation_events: Vec::new(),
        service_transitions: Vec::new(),
        graph_events: Vec::new(),
        post_start_hook: None,
        late_exit: Some(state),
    }
}
