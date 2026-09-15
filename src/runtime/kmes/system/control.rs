use crate::boundary::{BoundaryError, KmesEvent};
use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::control::reload_config::{ReloadConfigError, ReloadConfigOutcome};
use crate::kmes::{
    encode_graph_validation_error_event, encode_graph_validation_warning_event,
    encode_service_access_denied_event, encode_system_access_denied_event,
};
use crate::supervisor::{
    SupervisorControlCommandBodyError, SupervisorControlCommandDispatch,
    SupervisorControlConnectionTableTurn, SupervisorControlFrameTurn, SupervisorLifecycleDispatch,
    SupervisorSystemShutdownControlBodyError,
};

use super::super::event::{
    collect_on_demand_start, collect_operation_request_outcome, push_operation,
};
use super::super::job::collect_start_dispatches;
use super::shutdown::collect_system_shutdown_dispatch;

pub(in crate::runtime::kmes) fn collect_control_connection_table_turn(
    turn: &SupervisorControlConnectionTableTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    for frame in &turn.turn.frames {
        collect_control_frame_turn(&frame.frame, out)?;
    }
    Ok(())
}

fn collect_control_frame_turn(
    turn: &SupervisorControlFrameTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    match turn {
        SupervisorControlFrameTurn::ShutdownAccepted { dispatch, .. } => {
            collect_system_shutdown_dispatch(dispatch, out)?;
        }
        SupervisorControlFrameTurn::CommandAccepted {
            access_denials,
            job_access_denials,
            dispatch: Some(dispatch),
            ..
        } => {
            collect_access_denials(access_denials, out)?;
            collect_job_access_denials(job_access_denials, out)?;
            collect_control_command_dispatch(dispatch, out)?;
        }
        SupervisorControlFrameTurn::CommandAccepted {
            access_denials,
            job_access_denials,
            dispatch: None,
            ..
        } => {
            collect_access_denials(access_denials, out)?;
            collect_job_access_denials(job_access_denials, out)?;
        }
        SupervisorControlFrameTurn::ShutdownRejected { error, .. } => {
            collect_shutdown_rejection(error, out)?;
        }
        SupervisorControlFrameTurn::CommandRejected { error, .. } => {
            collect_control_command_rejection(error, out)?;
        }
        SupervisorControlFrameTurn::Incomplete { .. }
        | SupervisorControlFrameTurn::RejectedFrame { .. } => {}
    }
    Ok(())
}

fn collect_control_command_dispatch(
    dispatch: &SupervisorControlCommandDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    match dispatch {
        SupervisorControlCommandDispatch::Shutdown(dispatch) => {
            collect_system_shutdown_dispatch(dispatch, out)?;
        }
        SupervisorControlCommandDispatch::Lifecycle(dispatch) => {
            collect_lifecycle_dispatch(dispatch, out)?;
        }
        SupervisorControlCommandDispatch::ReloadConfig(outcome) => {
            collect_reload_config_warnings(outcome, out)?;
        }
        SupervisorControlCommandDispatch::Job(dispatch) => {
            super::super::submitted::collect_jobs_command_dispatch(dispatch, out)?;
        }
    }
    Ok(())
}

fn collect_control_command_rejection(
    error: &SupervisorControlCommandBodyError,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    match error {
        SupervisorControlCommandBodyError::SystemAccessDenied(denied) => {
            out.push(encode_system_access_denied_event(denied)?);
        }
        SupervisorControlCommandBodyError::ServiceAccessDenied(denied) => {
            out.push(encode_service_access_denied_event(denied)?);
        }
        SupervisorControlCommandBodyError::JobAccessDenied(denied) => {
            out.push(crate::kmes::encode_job_access_denied_event(denied)?);
        }
        SupervisorControlCommandBodyError::ReloadConfig(error) => {
            if let ReloadConfigError::Validation(failure) = error.as_ref() {
                for finding in &failure.findings {
                    out.push(encode_graph_validation_error_event(
                        "reload_config",
                        finding,
                    )?);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_job_access_denials(
    denials: &[crate::submitted::JobAccessDenied],
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    for denied in denials {
        out.push(crate::kmes::encode_job_access_denied_event(denied)?);
    }
    Ok(())
}

fn collect_access_denials(
    access_denials: &[crate::control::service_security::ServiceAccessDenied],
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    for denied in access_denials {
        out.push(encode_service_access_denied_event(denied)?);
    }
    Ok(())
}

fn collect_shutdown_rejection(
    error: &SupervisorSystemShutdownControlBodyError,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    if let SupervisorSystemShutdownControlBodyError::AccessDenied(denied) = error {
        out.push(encode_system_access_denied_event(denied)?);
    }
    Ok(())
}

fn collect_reload_config_warnings(
    outcome: &ReloadConfigOutcome,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    for warning in &outcome.warnings {
        out.push(encode_graph_validation_warning_event(
            "reload_config",
            warning,
        )?);
    }
    Ok(())
}

fn collect_lifecycle_dispatch(
    dispatch: &SupervisorLifecycleDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_lifecycle_outcome(&dispatch.outcome, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

fn collect_lifecycle_outcome(
    outcome: &LifecycleCommandOutcome,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    match outcome {
        LifecycleCommandOutcome::OperationAccepted(outcome) => {
            collect_operation_request_outcome(outcome, out)?;
        }
        LifecycleCommandOutcome::OnDemandStart(dispatch) => {
            collect_on_demand_start(dispatch, out)?;
        }
        LifecycleCommandOutcome::SynchronousClear(clear) => {
            collect_operation_request_outcome(&clear.request, out)?;
            push_operation(out, &clear.started)?;
            push_operation(out, &clear.completed)?;
        }
        LifecycleCommandOutcome::Already(_) | LifecycleCommandOutcome::Noop(_) => {}
    }
    Ok(())
}
