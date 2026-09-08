use crate::boundary::{ProcessSignal, ProcessTarget};
use crate::operation::store::OperationStore;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceDefinition, ServiceTable};

use super::super::command::parse_executable_command;
use super::command_reload::begin_command_reload;
use super::model::{
    ControlExecutionContext, ControlExecutionDispatch, ControlExecutionError,
    ControlOperationRequest, ReloadDetectionCompletion,
};
use super::signal::{begin_signal_reload, canonical_reload_signal};
use super::store::{ControlExecutionStore, ReloadDetectionDeadline, ReloadDetectionPhase};

pub(super) fn begin_reload_operation<P>(
    context: &mut ControlExecutionContext<'_, P>,
    request: ControlOperationRequest,
    target: ProcessTarget,
    definition: &ServiceDefinition,
) -> Result<ControlExecutionDispatch, ControlExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    match reload_action(&target.service, definition)? {
        ReloadAction::Signal(signal) => begin_signal_reload(context, request, target, signal),
        ReloadAction::Command(argv) => {
            begin_command_reload(context, request, target, definition, argv)
        }
    }
}

pub fn complete_reload_detection_window(
    services: &mut ServiceTable,
    operations: &mut OperationStore,
    control_store: &mut ControlExecutionStore,
    deadline: ReloadDetectionDeadline,
    now_ns: u64,
) -> Result<ReloadDetectionCompletion, ControlExecutionError> {
    let mut next_services = services.clone();
    let mut next_operations = operations.clone();
    let mut next_store = control_store.clone();

    next_store.remove_reload_detection_deadline(deadline.operation_id);
    let service_transition = next_services
        .transition_service(
            &deadline.service,
            ServiceTransition {
                to: ServiceState::Active,
                cause: TransitionCause::ExplicitReload,
            },
        )
        .map_err(ControlExecutionError::ServiceTable)?;
    let operation_event = next_operations
        .complete_operation(
            deadline.operation_id,
            now_ns,
            reload_detection_result(deadline.phase),
        )
        .map_err(ControlExecutionError::OperationStore)?;

    *services = next_services;
    *operations = next_operations;
    *control_store = next_store;

    Ok(ReloadDetectionCompletion {
        operation_event,
        service_transition,
        phase: deadline.phase,
    })
}

fn reload_detection_result(phase: ReloadDetectionPhase) -> &'static str {
    match phase {
        ReloadDetectionPhase::DetectionWindow => "reload signal advisory: detection window expired",
        ReloadDetectionPhase::ExtendedWait => {
            "reload signal advisory: service signalled RELOADING=1 but never completed reload"
        }
    }
}

enum ReloadAction {
    Signal(ProcessSignal),
    Command(Vec<String>),
}

fn reload_action(
    service: &str,
    definition: &ServiceDefinition,
) -> Result<ReloadAction, ControlExecutionError> {
    let Some(exec_reload) = definition.exec_reload.as_deref() else {
        return Ok(ReloadAction::Signal(ProcessSignal::Sighup));
    };
    if let Some(signal) = exec_reload.strip_prefix("signal:") {
        return canonical_reload_signal(service, signal).map(ReloadAction::Signal);
    }
    parse_executable_command(exec_reload)
        .map(ReloadAction::Command)
        .map_err(|source| ControlExecutionError::InvalidExecReloadCommand {
            service: service.to_string(),
            exec_reload: exec_reload.to_string(),
            source,
        })
}
