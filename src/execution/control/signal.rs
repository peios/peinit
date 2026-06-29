use crate::boundary::{ProcessSignal, ProcessTarget};
use crate::service::ServiceDefinition;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use super::model::{
    ControlExecutionContext, ControlExecutionDetail, ControlExecutionDispatch,
    ControlExecutionError, ControlOperationKind, ControlOperationRequest,
};
use super::store::{ReloadDetectionDeadline, ReloadDetectionPhase};

const NANOS_PER_SEC: u64 = 1_000_000_000;
const RELOAD_DETECTION_WINDOW_NS: u64 = 2 * NANOS_PER_SEC;

pub(super) fn begin_signal_reload<P>(
    context: &mut ControlExecutionContext<'_, P>,
    request: ControlOperationRequest,
    target: ProcessTarget,
    signal: ProcessSignal,
    definition: &ServiceDefinition,
) -> Result<ControlExecutionDispatch, ControlExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_store = context.control_store.clone();

    let operation_event = next_operations
        .start_operation(request.operation_id, request.observed_at_ns)
        .map_err(ControlExecutionError::OperationStore)?;
    let operation = next_operations
        .get(request.operation_id)
        .ok_or(
            crate::operation::store::OperationStoreError::UnknownOperation {
                id: request.operation_id,
            },
        )
        .map_err(ControlExecutionError::OperationStore)?
        .clone();
    let service_transition = next_services
        .transition_service(
            &target.service,
            ServiceTransition {
                to: ServiceState::Reloading,
                cause: TransitionCause::ExplicitReload,
            },
        )
        .map_err(ControlExecutionError::ServiceTable)?;
    let detection_deadline_ns = request
        .observed_at_ns
        .saturating_add(RELOAD_DETECTION_WINDOW_NS);
    let operation_deadline_ns = operation
        .created_at_ns
        .saturating_add(definition.start_timeout_secs.saturating_mul(NANOS_PER_SEC));
    let deadline_ns = detection_deadline_ns.min(operation_deadline_ns);

    context
        .controller
        .signal_main(&target, signal.clone())
        .map_err(ControlExecutionError::Boundary)?;
    next_store.record_reload_detection_deadline(ReloadDetectionDeadline {
        operation_id: request.operation_id,
        service: target.service.clone(),
        due_at_ns: deadline_ns,
        phase: ReloadDetectionPhase::DetectionWindow,
    });

    *context.services = next_services;
    *context.operations = next_operations;
    *context.control_store = next_store;

    Ok(ControlExecutionDispatch {
        operation_id: request.operation_id,
        service: target.service.clone(),
        kind: ControlOperationKind::ReloadSignal,
        operation_event,
        service_transition,
        detail: ControlExecutionDetail::Signal { target, signal },
        deadline_ns,
    })
}

pub(super) fn canonical_reload_signal(
    service: &str,
    signal: &str,
) -> Result<ProcessSignal, ControlExecutionError> {
    if signal == "SIGHUP" {
        return Ok(ProcessSignal::Sighup);
    }
    if accepted_reload_signals().contains(&signal) {
        return Ok(ProcessSignal::Named(signal.to_string()));
    }
    Err(ControlExecutionError::InvalidReloadSignal {
        service: service.to_string(),
        signal: signal.to_string(),
    })
}

fn accepted_reload_signals() -> &'static [&'static str] {
    &[
        "SIGINT",
        "SIGQUIT",
        "SIGILL",
        "SIGTRAP",
        "SIGABRT",
        "SIGBUS",
        "SIGFPE",
        "SIGUSR1",
        "SIGSEGV",
        "SIGUSR2",
        "SIGPIPE",
        "SIGALRM",
        "SIGTERM",
        "SIGSTKFLT",
        "SIGCHLD",
        "SIGCONT",
        "SIGTSTP",
        "SIGTTIN",
        "SIGTTOU",
        "SIGURG",
        "SIGXCPU",
        "SIGXFSZ",
        "SIGVTALRM",
        "SIGPROF",
        "SIGWINCH",
        "SIGIO",
        "SIGPWR",
        "SIGSYS",
    ]
}
