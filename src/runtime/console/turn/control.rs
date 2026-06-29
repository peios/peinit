use crate::control::lifecycle::LifecycleCommandOutcome;
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
    out: &mut Vec<String>,
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
    out: &mut Vec<String>,
) {
    if let Some(frame) = &turn.turn.frame {
        collect_control_frame_turn_console_messages(&frame.frame, out);
    }
}

pub(super) fn collect_notify_dispatch_console_messages(
    dispatch: &SupervisorNotifyDispatch,
    out: &mut Vec<String>,
) {
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

pub(super) fn collect_filesystem_check_helper_turn_console_messages(
    turn: &RuntimeFilesystemCheckHelperTurn,
    out: &mut Vec<String>,
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
    out: &mut Vec<String>,
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
    out: &mut Vec<String>,
) {
    match dispatch {
        SupervisorControlCommandDispatch::Shutdown(dispatch) => {
            collect_system_shutdown_dispatch_console_messages(dispatch, out);
        }
        SupervisorControlCommandDispatch::Lifecycle(dispatch) => {
            collect_lifecycle_dispatch_console_messages(dispatch, out);
        }
        SupervisorControlCommandDispatch::ReloadConfig(_) => {}
    }
}

fn collect_lifecycle_dispatch_console_messages(
    dispatch: &SupervisorLifecycleDispatch,
    out: &mut Vec<String>,
) {
    collect_lifecycle_outcome_console_messages(&dispatch.outcome, out);
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

fn collect_lifecycle_outcome_console_messages(
    outcome: &LifecycleCommandOutcome,
    out: &mut Vec<String>,
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
    out: &mut Vec<String>,
) {
    shutdown::collect_shutdown_dispatch_console_messages(&dispatch.shutdown, out);
}

fn collect_timer_dispatch_console_messages(
    dispatch: &SupervisorTimerDispatch,
    out: &mut Vec<String>,
) {
    if let SupervisorTimerAction::Start {
        start_dispatches, ..
    } = &dispatch.action
    {
        collect_start_dispatches_console_messages(start_dispatches, out);
    }
}
