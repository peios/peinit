use crate::boundary::{BoundaryError, KmesEvent};
use crate::kmes::{
    encode_fd_store_rejection_event, encode_notify_applied_field_events,
    encode_notify_rejection_event, encode_on_failure_loop_suppressed_event,
};
use crate::supervisor::{SupervisorFilesystemCheckCompletionDispatch, SupervisorNotifyDispatch};

use super::event::{push_graphs, push_job, push_operations};
use super::job::{
    collect_health_check_launch, collect_health_check_launch_failure,
    collect_post_start_hook_launch, collect_post_start_hook_launch_failure,
    collect_pre_start_check_completion, collect_service_launch, collect_service_launch_failure,
    collect_service_main_start_timeout, collect_start_dispatches, collect_start_hook_launch,
    collect_start_hook_launch_failure,
};
use super::system::{
    collect_child_reap_turn, collect_control_connection_table_turn,
    collect_lifecycle_deadline_dispatch, collect_pid1_signal_turn, collect_power_button_dispatch,
    collect_shutdown_drive_dispatch, collect_timer_dispatch,
};
use crate::runtime::{
    RuntimeCalendarTimerTurn, RuntimeFilesystemCheckHelperTurn, RuntimeNotifyRead,
    RuntimeNotifyRejection, RuntimeNotifySupervisorTurn, RuntimePowerButtonTurn,
    RuntimeProcessSetupTurn, RuntimeShutdownEventTurn, RuntimeWorkPumpTurn,
};

pub(crate) fn collect_runtime_loop_kmes_events(
    pre_work: &RuntimeWorkPumpTurn,
    maintenance_before_wait: &crate::supervisor::SupervisorOperationMaintenanceTurn,
    event_turns: &[RuntimeShutdownEventTurn],
    post_work: &RuntimeWorkPumpTurn,
    maintenance_after_sources: &crate::supervisor::SupervisorOperationMaintenanceTurn,
    calendar_turns: &[(i32, RuntimeCalendarTimerTurn)],
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    super::work::collect_runtime_work_pump_kmes_events(pre_work, out)?;
    collect_operation_maintenance_turn_kmes_events(maintenance_before_wait, out)?;
    for event_turn in event_turns {
        collect_runtime_shutdown_turn_kmes_events(event_turn, out)?;
    }
    super::work::collect_runtime_work_pump_kmes_events(post_work, out)?;
    collect_operation_maintenance_turn_kmes_events(maintenance_after_sources, out)?;
    for (_, calendar_turn) in calendar_turns {
        collect_runtime_calendar_timer_kmes_events(calendar_turn, out)?;
    }
    Ok(())
}

pub(crate) fn collect_runtime_shutdown_turn_kmes_events(
    turn: &RuntimeShutdownEventTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    match turn {
        RuntimeShutdownEventTurn::Pid1Signal {
            supervisor,
            child_reaps,
            drive,
            ..
        } => {
            collect_pid1_signal_turn(supervisor, out)?;
            for reap in child_reaps {
                collect_child_reap_turn(reap, out)?;
            }
            if let Some(drive) = drive {
                collect_shutdown_drive_dispatch(drive, out)?;
            }
        }
        RuntimeShutdownEventTurn::ControlConnection { supervisor, .. } => {
            collect_control_connection_table_turn(supervisor, out)?;
        }
        RuntimeShutdownEventTurn::ShutdownDeadlineTimer { drive, .. } => {
            if let Some(drive) = drive {
                collect_shutdown_drive_dispatch(drive, out)?;
            }
        }
        RuntimeShutdownEventTurn::LifecycleDeadlineTimer { drive, .. } => {
            if let Some(drive) = drive {
                collect_lifecycle_deadline_dispatch(drive, out)?;
            }
        }
        RuntimeShutdownEventTurn::Notify {
            read, supervisor, ..
        } => {
            if let Some(supervisor) = supervisor {
                collect_notify_supervisor_turn(read, supervisor, out)?;
            }
        }
        RuntimeShutdownEventTurn::CalendarTimer { turn, .. } => {
            collect_runtime_calendar_timer_kmes_events(turn, out)?;
        }
        RuntimeShutdownEventTurn::FilesystemCheckHelper { turn, .. }
        | RuntimeShutdownEventTurn::FilesystemCheckHelperExit { turn, .. } => {
            collect_filesystem_check_helper_turn(turn, out)?;
        }
        RuntimeShutdownEventTurn::ProcessSetup { turn, .. } => {
            collect_process_setup_turn(turn, out)?;
        }
        RuntimeShutdownEventTurn::PowerButton { turn, .. } => {
            collect_power_button_turn(turn, out)?;
        }
        RuntimeShutdownEventTurn::ControlListener { .. }
        | RuntimeShutdownEventTurn::IdleControlConnectionsClosed { .. }
        | RuntimeShutdownEventTurn::StaleControlConnection { .. }
        | RuntimeShutdownEventTurn::ServiceLogPipe { .. }
        | RuntimeShutdownEventTurn::JfsDevice { .. }
        | RuntimeShutdownEventTurn::RegistryWatch { .. } => {}
    }
    Ok(())
}

fn collect_power_button_turn(
    turn: &RuntimePowerButtonTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    let RuntimePowerButtonTurn::Shutdown { supervisor, .. } = turn else {
        return Ok(());
    };
    collect_power_button_dispatch(supervisor, out)
}

fn collect_process_setup_turn(
    turn: &RuntimeProcessSetupTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    let RuntimeProcessSetupTurn::Completed { supervisor, .. } = turn else {
        return Ok(());
    };
    match &**supervisor {
        crate::supervisor::SupervisorProcessSetupDispatch::ServiceMainLaunched(dispatch) => {
            collect_service_launch(dispatch, out)
        }
        crate::supervisor::SupervisorProcessSetupDispatch::ServiceMainFailed(dispatch) => {
            collect_service_launch_failure(dispatch, out)
        }
        crate::supervisor::SupervisorProcessSetupDispatch::StartHookLaunched(dispatch) => {
            collect_start_hook_launch(dispatch, out)
        }
        crate::supervisor::SupervisorProcessSetupDispatch::StartHookFailed(dispatch) => {
            collect_start_hook_launch_failure(dispatch, out)
        }
        crate::supervisor::SupervisorProcessSetupDispatch::PostHookLaunched(dispatch) => {
            collect_post_start_hook_launch(dispatch, out)
        }
        crate::supervisor::SupervisorProcessSetupDispatch::PostHookFailed(dispatch) => {
            collect_post_start_hook_launch_failure(dispatch, out)
        }
        crate::supervisor::SupervisorProcessSetupDispatch::ControlLaunched(dispatch) => {
            super::job::collect_launch(&dispatch.launch, out)
        }
        crate::supervisor::SupervisorProcessSetupDispatch::HealthCheckLaunched(dispatch) => {
            collect_health_check_launch(dispatch, out)
        }
        crate::supervisor::SupervisorProcessSetupDispatch::HealthCheckFailed(dispatch) => {
            collect_health_check_launch_failure(dispatch, out)
        }
        crate::supervisor::SupervisorProcessSetupDispatch::ControlTimeout(_)
        | crate::supervisor::SupervisorProcessSetupDispatch::Pending(_)
        | crate::supervisor::SupervisorProcessSetupDispatch::Stale { .. } => Ok(()),
    }
}

pub(crate) fn collect_runtime_calendar_timer_kmes_events(
    turn: &RuntimeCalendarTimerTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    if let RuntimeCalendarTimerTurn::Read {
        supervisor: Some(dispatch),
        ..
    } = turn
    {
        collect_timer_dispatch(dispatch, out)?;
    }
    Ok(())
}

pub(crate) fn collect_operation_maintenance_turn_kmes_events(
    turn: &crate::supervisor::SupervisorOperationMaintenanceTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    push_operations(out, &turn.operation_timeouts)?;
    for timeout in &turn.service_main_start_timeouts {
        collect_service_main_start_timeout(timeout, out)?;
    }
    push_graphs(out, &turn.graph_events)?;
    for event in &turn.relationship_audit_events {
        out.push(encode_on_failure_loop_suppressed_event(event)?);
    }
    collect_start_dispatches(&turn.start_dispatches, out)
}

fn collect_notify_supervisor_turn(
    read: &RuntimeNotifyRead,
    turn: &RuntimeNotifySupervisorTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    match turn {
        RuntimeNotifySupervisorTurn::Applied(dispatch) => collect_notify_dispatch(dispatch, out),
        RuntimeNotifySupervisorTurn::Rejected(rejection) => {
            let sender_pid = notify_sender_pid(read);
            let attribution = notify_rejection_attribution(rejection);
            out.push(encode_notify_rejection_event(
                sender_pid,
                &notify_rejection_reason(rejection),
                attribution,
            )?);
            Ok(())
        }
    }
}

fn collect_notify_dispatch(
    dispatch: &SupervisorNotifyDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    out.extend(encode_notify_applied_field_events(
        &dispatch.notify.sender,
        &dispatch.notify.applied_fields,
    )?);
    push_operations(out, &dispatch.notify.operation_events)?;
    push_graphs(out, &dispatch.notify.graph_events)?;
    if let Some(job_event) = &dispatch.notify.post_start_hook {
        push_job(out, job_event)?;
    }
    for rejection in &dispatch.fd_store_rejections {
        out.push(encode_fd_store_rejection_event(rejection)?);
    }
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

fn collect_filesystem_check_helper_turn(
    turn: &RuntimeFilesystemCheckHelperTurn,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    match turn {
        RuntimeFilesystemCheckHelperTurn::Completed { completion }
        | RuntimeFilesystemCheckHelperTurn::ReadFailedClosed { completion, .. } => {
            collect_filesystem_check_completion(completion, out)?;
        }
        RuntimeFilesystemCheckHelperTurn::WouldBlock { .. }
        | RuntimeFilesystemCheckHelperTurn::Stale { .. } => {}
    }
    Ok(())
}

fn collect_filesystem_check_completion(
    dispatch: &SupervisorFilesystemCheckCompletionDispatch,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    collect_pre_start_check_completion(&dispatch.completion, out)?;
    collect_start_dispatches(&dispatch.start_dispatches, out)
}

fn notify_sender_pid(read: &RuntimeNotifyRead) -> Option<u32> {
    match read {
        RuntimeNotifyRead::Datagram(datagram) => Some(datagram.sender_pid),
        RuntimeNotifyRead::WouldBlock => None,
    }
}

fn notify_rejection_attribution(
    rejection: &RuntimeNotifyRejection,
) -> Option<&crate::execution::notify::AuthenticatedNotifySender> {
    match rejection {
        RuntimeNotifyRejection::Parse { attribution, .. }
        | RuntimeNotifyRejection::Apply { attribution, .. } => attribution.as_ref(),
        RuntimeNotifyRejection::Shutdown(_) | RuntimeNotifyRejection::Truncated { .. } => None,
    }
}

fn notify_rejection_reason(rejection: &RuntimeNotifyRejection) -> String {
    match rejection {
        RuntimeNotifyRejection::Parse { error, .. } => format!("parse: {error:?}"),
        RuntimeNotifyRejection::Apply { error, .. } => format!("apply: {error:?}"),
        RuntimeNotifyRejection::Shutdown(error) => format!("shutdown: {error:?}"),
        RuntimeNotifyRejection::Truncated { payload, control } => {
            format!("truncated: payload={payload} control={control}")
        }
    }
}
