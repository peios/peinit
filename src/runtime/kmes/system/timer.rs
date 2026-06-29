use crate::boundary::{BoundaryError, KmesEvent};
use crate::supervisor::{SupervisorTimerAction, SupervisorTimerDispatch};

use super::super::event::collect_on_demand_start;
use super::super::job::collect_start_dispatches;

pub(in crate::runtime::kmes) fn collect_timer_dispatch(
    dispatch: &SupervisorTimerDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    if let SupervisorTimerAction::Start {
        outcome,
        start_dispatches,
        ..
    } = &dispatch.action
    {
        collect_on_demand_start(outcome, out)?;
        collect_start_dispatches(start_dispatches, out)?;
    }
    Ok(())
}
