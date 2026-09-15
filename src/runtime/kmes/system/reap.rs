use crate::boundary::{BoundaryError, KmesEvent};
use crate::supervisor::{SupervisorChildReapDispatch, SupervisorChildReapTurn};

use super::super::job::{
    collect_health_check_terminal, collect_post_start_hook_terminal_dispatch,
    collect_pre_start_hook_terminal_dispatch, collect_reload_command_terminal,
    collect_terminal_dispatch,
};
use super::shutdown::collect_shutdown_terminal_dispatch;

pub(in crate::runtime::kmes) fn collect_child_reap_turn(
    turn: &SupervisorChildReapTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    let dispatch = match turn {
        SupervisorChildReapTurn::Tracked { dispatch, .. } => dispatch,
        SupervisorChildReapTurn::InternalError { dispatch, .. } => {
            return super::super::event::push_internal_error(out, dispatch);
        }
        SupervisorChildReapTurn::Untracked { .. }
        | SupervisorChildReapTurn::DeferredUntilSetup { .. } => return Ok(()),
    };
    match dispatch {
        SupervisorChildReapDispatch::Runtime(dispatch) => collect_terminal_dispatch(dispatch, out)?,
        SupervisorChildReapDispatch::PreStartHook(dispatch) => {
            collect_pre_start_hook_terminal_dispatch(dispatch, out)?;
        }
        SupervisorChildReapDispatch::PostStartHook(dispatch) => {
            collect_post_start_hook_terminal_dispatch(dispatch, out)?;
        }
        SupervisorChildReapDispatch::ReloadCommand(dispatch) => {
            collect_reload_command_terminal(&dispatch.terminal, out)?;
        }
        SupervisorChildReapDispatch::HealthCheck(dispatch) => {
            collect_health_check_terminal(dispatch, out)?;
        }
        SupervisorChildReapDispatch::Shutdown(dispatch) => {
            collect_shutdown_terminal_dispatch(dispatch, out)?;
        }
        SupervisorChildReapDispatch::Submitted(dispatch) => {
            super::super::submitted::collect_submitted_terminal(dispatch, out)?;
        }
    }
    Ok(())
}
