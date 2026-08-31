use crate::boundary::{ProcessSignal, ProcessTarget};
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
    let service_transition = next_services
        .transition_service(
            &target.service,
            ServiceTransition {
                to: ServiceState::Reloading,
                cause: TransitionCause::ExplicitReload,
            },
        )
        .map_err(ControlExecutionError::ServiceTable)?;
    // §5.3: the detection window is two seconds, "fixed and not configurable
    // via the registry". It used to be clamped to the operation's own
    // StartTimeout-derived deadline, which made the constant a ceiling rather
    // than a value and made it indirectly registry-configurable: a service with
    // `StartTimeout` under two seconds got a shorter window, for no reason
    // connected to reloading (PEI-359).
    //
    // Nothing is lost by dropping the clamp. The reload operation's own
    // lifetime still bounds the whole thing, and a detection window that
    // outlives it simply resolves against an operation that has already
    // finished.
    let deadline_ns = request
        .observed_at_ns
        .saturating_add(RELOAD_DETECTION_WINDOW_NS);

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
