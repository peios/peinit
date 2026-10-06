use crate::boundary::BoundaryError;
use crate::control::lifecycle::LifecycleCommandOutcome;
use crate::control::reload_config::{ReloadConfigError, ReloadConfigOutcome};
use crate::kmes::types::{
    CONFIG_RELOAD_APPLIED, CONFIG_RELOAD_DEFERRED, GRAPH_VALIDATION_FAILED,
    GRAPH_VALIDATION_WARNED,
};
use crate::kmes::{
    EventCollector, GraphPhase, encode_config_reload_applied_event,
    encode_graph_validation_error_event, encode_graph_validation_warning_event,
    encode_reload_undecodable_service_event,
};
use crate::supervisor::{
    SupervisorControlCommandBodyError, SupervisorControlCommandDispatch,
    SupervisorControlConnectionTableTurn, SupervisorControlFrameTurn, SupervisorLifecycleDispatch,
};

use super::super::event::{
    collect_on_demand_start, collect_operation_request_outcome, push_operation,
};
use super::super::job::collect_start_dispatches;
use super::shutdown::collect_system_shutdown_dispatch;

pub(in crate::runtime::kmes) fn collect_control_connection_table_turn(
    turn: &SupervisorControlConnectionTableTurn,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    for frame in &turn.turn.frames {
        collect_control_frame_turn(&frame.frame, out)?;
    }
    Ok(())
}

/// A command's events. A refusal by a descriptor writes none of peinit's
/// own: the decision is recorded by KACS, as `kacs.audit.access.checked`,
/// when the descriptor's SACL asks for it (PEI-1279).
fn collect_control_frame_turn(
    turn: &SupervisorControlFrameTurn,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    match turn {
        SupervisorControlFrameTurn::ShutdownAccepted { dispatch, .. } => {
            collect_system_shutdown_dispatch(dispatch, out)?;
        }
        SupervisorControlFrameTurn::CommandAccepted {
            dispatch: Some(dispatch),
            ..
        } => {
            collect_control_command_dispatch(dispatch, out)?;
        }
        SupervisorControlFrameTurn::CommandRejected { error, .. } => {
            collect_control_command_rejection(error, out)?;
        }
        SupervisorControlFrameTurn::CommandAccepted { dispatch: None, .. }
        | SupervisorControlFrameTurn::ShutdownRejected { .. }
        | SupervisorControlFrameTurn::Incomplete { .. }
        | SupervisorControlFrameTurn::RejectedFrame { .. } => {}
    }
    Ok(())
}

fn collect_control_command_dispatch(
    dispatch: &SupervisorControlCommandDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    match dispatch {
        SupervisorControlCommandDispatch::Shutdown(dispatch) => {
            collect_system_shutdown_dispatch(dispatch, out)?;
        }
        SupervisorControlCommandDispatch::Lifecycle(dispatch) => {
            collect_lifecycle_dispatch(dispatch, out)?;
        }
        SupervisorControlCommandDispatch::ReloadConfig(outcome) => {
            // An explicit reload records that it applied, as the reload
            // after the boot window does, and then what it found.
            out.push(CONFIG_RELOAD_APPLIED, |_| {
                encode_config_reload_applied_event(None, Ok(outcome))
            })?;
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
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    if let SupervisorControlCommandBodyError::ReloadConfig(error) = error {
        collect_reload_config_failure(error, out)?;
    }
    Ok(())
}

/// An explicit reload that failed: that it did, and each finding of a
/// validation that rejected it.
fn collect_reload_config_failure(
    error: &ReloadConfigError,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    out.push(CONFIG_RELOAD_APPLIED, |_| {
        encode_config_reload_applied_event(None, Err(error))
    })?;
    if let ReloadConfigError::Validation(failure) = error {
        for finding in &failure.findings {
            out.push(GRAPH_VALIDATION_FAILED, |_| {
                encode_graph_validation_error_event(GraphPhase::ReloadConfig, finding)
            })?;
        }
    }
    Ok(())
}

pub(in crate::runtime::kmes) fn collect_reload_config_warnings(
    outcome: &ReloadConfigOutcome,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    // A boot-plan member the reload could not touch yet: the audit trail
    // records that the change is pending on the boot window (PEI-350).
    if !outcome.summary.deferred.is_empty() {
        out.push(CONFIG_RELOAD_DEFERRED, |_| {
            crate::kmes::encode_registry_reload_deferred_event(&outcome.summary.deferred)
        })?;
    }
    // A key that would not decode has failed its service (§2.5), and that
    // is a validation error for the audit trail exactly as it is at boot
    // (PEI-621).
    for service in &outcome.undecodable {
        out.push(GRAPH_VALIDATION_FAILED, |_| {
            encode_reload_undecodable_service_event(service)
        })?;
    }
    for warning in &outcome.warnings {
        out.push(GRAPH_VALIDATION_WARNED, |_| {
            encode_graph_validation_warning_event(GraphPhase::ReloadConfig, warning)
        })?;
    }
    Ok(())
}

fn collect_lifecycle_dispatch(
    dispatch: &SupervisorLifecycleDispatch,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    collect_lifecycle_outcome(&dispatch.outcome, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

fn collect_lifecycle_outcome(
    outcome: &LifecycleCommandOutcome,
    out: &mut EventCollector<'_>,
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
