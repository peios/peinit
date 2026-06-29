use crate::boundary::{BoundaryError, KmesEvent};
use crate::runtime::RuntimeWorkPumpTurn;

use super::job::{
    collect_control_dispatch, collect_health_check_launch,
    collect_health_check_launch_cancellation, collect_health_check_launch_failure, collect_launch,
    collect_post_start_hook_launch, collect_post_start_hook_launch_failure, collect_service_launch,
    collect_service_launch_failure, collect_start_hook_launch, collect_start_hook_launch_failure,
};

pub(crate) fn collect_runtime_work_pump_kmes_events(
    turn: &RuntimeWorkPumpTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    for dispatch in &turn.control_operations {
        collect_control_dispatch(dispatch, out)?;
    }
    for dispatch in &turn.start_hook_launches {
        collect_start_hook_launch(dispatch, out)?;
    }
    for dispatch in &turn.start_hook_launch_failures {
        collect_start_hook_launch_failure(dispatch, out)?;
    }
    for dispatch in &turn.post_hook_launches {
        collect_post_start_hook_launch(dispatch, out)?;
    }
    for dispatch in &turn.post_hook_launch_failures {
        collect_post_start_hook_launch_failure(dispatch, out)?;
    }
    for dispatch in &turn.control_launches {
        collect_launch(&dispatch.launch, out)?;
    }
    for dispatch in &turn.health_check_launches {
        collect_health_check_launch(dispatch, out)?;
    }
    for dispatch in &turn.health_check_launch_failures {
        collect_health_check_launch_failure(dispatch, out)?;
    }
    for dispatch in &turn.health_check_launch_cancellations {
        collect_health_check_launch_cancellation(dispatch, out)?;
    }
    for dispatch in &turn.service_launches {
        collect_service_launch(dispatch, out)?;
    }
    for dispatch in &turn.service_launch_failures {
        collect_service_launch_failure(dispatch, out)?;
    }
    Ok(())
}
