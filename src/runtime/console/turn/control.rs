use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::runtime::console::ConsoleMessage;
use crate::runtime::{RuntimeCalendarTimerTurn, RuntimeFilesystemCheckHelperTurn};
use crate::supervisor::{
    SupervisorControlCommandDispatch, SupervisorControlConnectionTableTurn,
    SupervisorControlFrameTurn, SupervisorLifecycleDispatch, SupervisorNotifyDispatch,
    SupervisorSystemShutdownDispatch, SupervisorTimerAction, SupervisorTimerDispatch,
};

use super::shutdown;
use crate::runtime::console::{
    collect_service_transition_console_message, collect_service_transitions_console_messages,
    collect_start_dispatches_console_messages,
};

pub(super) fn collect_runtime_calendar_timer_console_messages(
    turn: &RuntimeCalendarTimerTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    if let RuntimeCalendarTimerTurn::Read {
        supervisor: Some(dispatch),
        ..
    } = turn
    {
        collect_timer_dispatch_console_messages(dispatch, out);
    }
}

pub(super) fn collect_control_connection_table_turn_console_messages(
    turn: &SupervisorControlConnectionTableTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    for frame in &turn.turn.frames {
        collect_control_frame_turn_console_messages(&frame.frame, out);
    }
}

pub(super) fn collect_notify_dispatch_console_messages(
    dispatch: &SupervisorNotifyDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

pub(super) fn collect_filesystem_check_helper_turn_console_messages(
    turn: &RuntimeFilesystemCheckHelperTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    match turn {
        RuntimeFilesystemCheckHelperTurn::Completed { completion }
        | RuntimeFilesystemCheckHelperTurn::ReadFailedClosed { completion, .. } => {
            collect_service_transitions_console_messages(
                &completion.completion.service_transitions,
                out,
            );
            collect_start_dispatches_console_messages(&completion.start_dispatches, out);
        }
        RuntimeFilesystemCheckHelperTurn::WouldBlock { .. }
        | RuntimeFilesystemCheckHelperTurn::Stale { .. } => {}
    }
}

fn collect_control_frame_turn_console_messages(
    turn: &SupervisorControlFrameTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    match turn {
        SupervisorControlFrameTurn::ShutdownAccepted { dispatch, .. } => {
            collect_system_shutdown_dispatch_console_messages(dispatch, out);
        }
        SupervisorControlFrameTurn::CommandAccepted {
            dispatch: Some(dispatch),
            ..
        } => collect_control_command_dispatch_console_messages(dispatch, out),
        SupervisorControlFrameTurn::Incomplete { .. }
        | SupervisorControlFrameTurn::RejectedFrame { .. }
        | SupervisorControlFrameTurn::ShutdownRejected { .. }
        | SupervisorControlFrameTurn::CommandAccepted { dispatch: None, .. }
        | SupervisorControlFrameTurn::CommandRejected { .. } => {}
    }
}

fn collect_control_command_dispatch_console_messages(
    dispatch: &SupervisorControlCommandDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    match dispatch {
        SupervisorControlCommandDispatch::Shutdown(dispatch) => {
            collect_system_shutdown_dispatch_console_messages(dispatch, out);
        }
        SupervisorControlCommandDispatch::Lifecycle(dispatch) => {
            collect_lifecycle_dispatch_console_messages(dispatch, out);
        }
        SupervisorControlCommandDispatch::ReloadConfig(outcome) => {
            collect_reload_config_console_messages(outcome, out);
        }
        SupervisorControlCommandDispatch::Job(_) => {}
    }
}

/// What a reload has to say beyond its answer to one svctl, because this is
/// what everyone else sees: a service failed over a key that would not
/// decode, said as a boot says it for a blocked service (PEI-621); and the
/// boot-plan members the reload could not touch yet, so an operator watching
/// the boot knows why a `reg apply` from an install script has not reached
/// them (PEI-350).
pub(super) fn collect_reload_config_console_messages(
    outcome: &crate::control::reload_config::ReloadConfigOutcome,
    out: &mut Vec<ConsoleMessage>,
) {
    if !outcome.summary.deferred.is_empty() {
        crate::runtime::console::push_message(
            out,
            format!(
                "peinit: registry changed during the boot window; {} definition(s) deferred \
                 until the boot plan drains: {}\n",
                outcome.summary.deferred.len(),
                outcome.summary.deferred.join(", "),
            ),
        );
    }
    for service in &outcome.undecodable {
        crate::runtime::console::push_error(
            out,
            format!(
                "peinit: service {} failed: ValidationError ({})\n",
                service.name,
                crate::service::undecodable_message(service),
            ),
        );
    }
}

fn collect_lifecycle_dispatch_console_messages(
    dispatch: &SupervisorLifecycleDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    collect_lifecycle_outcome_console_messages(&dispatch.outcome, out);
    // The reset succeeded and the service is back to Inactive, so nothing in
    // the transitions above says the cgroup is still leaked. An operator who
    // issues the reset without reading its acknowledgement would otherwise
    // never learn that.
    for warning in &dispatch.lifecycle_warnings {
        crate::runtime::console::push_error(out, format!("peinit warning: {warning}\n"));
    }
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

fn collect_lifecycle_outcome_console_messages(
    outcome: &LifecycleCommandOutcome,
    out: &mut Vec<ConsoleMessage>,
) {
    match outcome {
        LifecycleCommandOutcome::SynchronousClear(clear) => {
            collect_service_transition_console_message(&clear.service_transition, out);
        }
        LifecycleCommandOutcome::OperationAccepted(_)
        | LifecycleCommandOutcome::OnDemandStart(_)
        | LifecycleCommandOutcome::Already(_)
        | LifecycleCommandOutcome::Noop(_) => {}
    }
}

fn collect_system_shutdown_dispatch_console_messages(
    dispatch: &SupervisorSystemShutdownDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    shutdown::collect_shutdown_dispatch_console_messages(&dispatch.shutdown, out);
}

fn collect_timer_dispatch_console_messages(
    dispatch: &SupervisorTimerDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    if let SupervisorTimerAction::Start {
        start_dispatches, ..
    } = &dispatch.action
    {
        collect_start_dispatches_console_messages(start_dispatches, out);
    }
}

/// The one reload that followed the boot window: what it did, so the
/// operator who saw the deferral sees it land (PEI-350). Each undecodable
/// key is reported as it is for any other reload.
pub(super) fn collect_deferred_registry_reload_console_messages(
    turn: &crate::runtime::RuntimeDeferredRegistryReloadTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    match turn.outcome.as_ref() {
        Ok(outcome) => {
            let summary = &outcome.summary;
            crate::runtime::console::push_ok(
                out,
                format!(
                    "peinit: configuration reloaded after the boot plan drained \
                     ({} definition(s) deferred): \
                     added {}, updated {}, restored {}, marked removed {}, discarded {}, \
                     undecodable {}\n",
                    turn.deferred.services.len(),
                    summary.added.len(),
                    summary.updated.len(),
                    summary.restored.len(),
                    summary.marked_removed.len(),
                    summary.discarded.len(),
                    summary.undecodable.len(),
                ),
            );
            collect_reload_config_console_messages(outcome, out);
        }
        Err(error) => {
            crate::runtime::console::push_error(
                out,
                format!(
                    "peinit warning: the configuration reload deferred by the boot window \
                     failed: {error:?}\n"
                ),
            );
        }
    }
}
