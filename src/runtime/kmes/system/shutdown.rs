use crate::boundary::{BoundaryError, KmesEvent};
use crate::supervisor::{
    SupervisorPowerButtonAction, SupervisorPowerButtonDispatch, SupervisorShutdownDispatch,
    SupervisorShutdownDriveDispatch, SupervisorShutdownSignalAction,
    SupervisorShutdownSignalDispatch, SupervisorShutdownTerminalDispatch,
    SupervisorSystemShutdownDispatch,
};

use super::super::event::{push_job, push_operations, push_shutdown_abandoned};

pub(in crate::runtime::kmes) fn collect_pid1_signal_turn(
    turn: &crate::supervisor::SupervisorPid1SignalFdTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    let crate::supervisor::SupervisorPid1SignalFdTurn::Shutdown(dispatch) = turn else {
        return Ok(());
    };
    collect_shutdown_signal_dispatch(dispatch, out)
}

pub(in crate::runtime::kmes) fn collect_power_button_dispatch(
    dispatch: &SupervisorPowerButtonDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    match &dispatch.action {
        SupervisorPowerButtonAction::Graceful(shutdown) => {
            collect_shutdown_dispatch(shutdown, out)?;
        }
        SupervisorPowerButtonAction::AlreadyInProgress { .. } => {}
    }
    Ok(())
}

pub(in crate::runtime::kmes) fn collect_shutdown_drive_dispatch(
    dispatch: &SupervisorShutdownDriveDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    if let Some(timeout) = &dispatch.timeout {
        for job_event in &timeout.job_events {
            push_job(out, job_event)?;
        }
        for abandoned in &timeout.abandoned {
            push_shutdown_abandoned(out, abandoned)?;
        }
        for dispatch in &timeout.submitted {
            super::super::submitted::collect_submitted_deadline(dispatch, out)?;
        }
    }
    Ok(())
}

pub(in crate::runtime::kmes::system) fn collect_system_shutdown_dispatch(
    dispatch: &SupervisorSystemShutdownDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_shutdown_dispatch(&dispatch.shutdown, out)
}

pub(in crate::runtime::kmes::system) fn collect_shutdown_terminal_dispatch(
    dispatch: &SupervisorShutdownTerminalDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_job(out, &dispatch.job_event)
}

fn collect_shutdown_signal_dispatch(
    dispatch: &SupervisorShutdownSignalDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    match &dispatch.action {
        SupervisorShutdownSignalAction::Graceful(shutdown) => {
            collect_shutdown_dispatch(shutdown, out)?;
        }
        SupervisorShutdownSignalAction::Forced(_)
        | SupervisorShutdownSignalAction::AlreadyInProgress { .. } => {}
    }
    Ok(())
}

fn collect_shutdown_dispatch(
    dispatch: &SupervisorShutdownDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_operations(out, &dispatch.startup_operation_events)?;
    for job_event in &dispatch.startup_job_events {
        push_job(out, job_event)?;
    }
    Ok(())
}
