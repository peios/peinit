use crate::boundary::{KmesEventSink, ProcessController};
use crate::control::connection::{
    ControlConnectionIo, ControlConnectionRecord, ControlConnectionTable,
};
#[cfg(feature = "peios-registry")]
use crate::runtime::RuntimeRegistryWatchTurn;
use crate::runtime::{
    RuntimeCalendarTimerTurn, RuntimeShutdownEventTurn, RuntimeShutdownLoopError,
    RuntimeShutdownLoopTurn, RuntimeWorkPumpTurn, collect_runtime_loop_kmes_events,
};
use crate::supervisor::{Supervisor, SupervisorOperationMaintenanceTurn};
#[cfg(feature = "peios-registry")]
use crate::supervisor::{SupervisorControlCommandDispatch, SupervisorControlFrameTurn};

#[cfg(feature = "peios-registry")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime::linux::turn) enum CalendarTimerMaintenanceStep {
    ProcessReadyTimers,
    ReconfigureAfterReload,
}

pub(in crate::runtime::linux::turn) fn deadline_wait_timeout_ms(
    deadline_ns: Option<u64>,
    now_ns: u64,
) -> i32 {
    let Some(deadline_ns) = deadline_ns else {
        return -1;
    };
    if deadline_ns <= now_ns {
        return 0;
    }
    let remaining_ns = deadline_ns - now_ns;
    let remaining_ms = remaining_ns.div_ceil(1_000_000);
    remaining_ms.min(i32::MAX as u64) as i32
}

pub(in crate::runtime::linux::turn) fn process_due_operation_maintenance_at(
    supervisor: &mut Supervisor,
    controller: &mut dyn ProcessController,
    now_ns: u64,
) -> Result<SupervisorOperationMaintenanceTurn, RuntimeShutdownLoopError> {
    if supervisor
        .next_operation_maintenance_deadline_ns()
        .is_some_and(|deadline_ns| deadline_ns <= now_ns)
    {
        return supervisor
            .process_due_operation_maintenance_with_controller(controller, now_ns)
            .map_err(RuntimeShutdownLoopError::OperationMaintenance);
    }
    Ok(SupervisorOperationMaintenanceTurn::default())
}

pub(in crate::runtime::linux::turn) fn flush_operation_waits_at<I>(
    supervisor: &Supervisor,
    control_connections: &mut ControlConnectionTable<ControlConnectionRecord<I>>,
    now_ns: u64,
) -> Result<(), RuntimeShutdownLoopError>
where
    I: ControlConnectionIo,
{
    supervisor
        .flush_terminal_control_waits(control_connections, now_ns)
        .map(|_| ())
        .map_err(RuntimeShutdownLoopError::ControlWait)
}

pub(in crate::runtime::linux::turn) fn prepend_idle_closure_turn(
    turn: &mut RuntimeShutdownLoopTurn,
    fds: Vec<i32>,
) {
    if !fds.is_empty() {
        turn.turns.insert(
            0,
            RuntimeShutdownEventTurn::IdleControlConnectionsClosed { fds },
        );
    }
}

pub(in crate::runtime::linux::turn) fn append_idle_closure_turn(
    turn: &mut RuntimeShutdownLoopTurn,
    fds: Vec<i32>,
) {
    if !fds.is_empty() {
        turn.turns
            .push(RuntimeShutdownEventTurn::IdleControlConnectionsClosed { fds });
    }
}

pub(in crate::runtime::linux::turn) fn emit_runtime_loop_kmes_events(
    sink: &mut dyn KmesEventSink,
    pre_work: &RuntimeWorkPumpTurn,
    maintenance_before_wait: &SupervisorOperationMaintenanceTurn,
    event_turns: &[RuntimeShutdownEventTurn],
    post_work: &RuntimeWorkPumpTurn,
    maintenance_after_sources: &SupervisorOperationMaintenanceTurn,
    calendar_turns: &[(i32, RuntimeCalendarTimerTurn)],
) -> Result<(), RuntimeShutdownLoopError> {
    let mut events = Vec::new();
    collect_runtime_loop_kmes_events(
        pre_work,
        maintenance_before_wait,
        event_turns,
        post_work,
        maintenance_after_sources,
        calendar_turns,
        &mut events,
    )
    .map_err(RuntimeShutdownLoopError::Kmes)?;
    sink.emit_kmes_events(&events)
        .map_err(RuntimeShutdownLoopError::Kmes)
}

#[cfg(feature = "peios-registry")]
pub(in crate::runtime::linux::turn) fn calendar_timer_maintenance_steps(
    reload_config_succeeded: bool,
) -> Vec<CalendarTimerMaintenanceStep> {
    let mut steps = vec![CalendarTimerMaintenanceStep::ProcessReadyTimers];
    if reload_config_succeeded {
        steps.push(CalendarTimerMaintenanceStep::ReconfigureAfterReload);
    }
    steps
}

#[cfg(feature = "peios-registry")]
pub(in crate::runtime::linux::turn) fn reload_config_succeeded(
    turns: &[RuntimeShutdownEventTurn],
) -> bool {
    turns.iter().any(|turn| match turn {
        RuntimeShutdownEventTurn::RegistryWatch {
            turn: RuntimeRegistryWatchTurn::ReloadConfig { outcome, .. },
            ..
        } => outcome.is_ok(),
        RuntimeShutdownEventTurn::ControlConnection {
            supervisor: supervisor_turn,
            ..
        } => matches!(
            supervisor_turn
                .turn
                .frame
                .as_ref()
                .map(|frame| &frame.frame),
            Some(SupervisorControlFrameTurn::CommandAccepted {
                dispatch: Some(dispatch),
                ..
            }) if matches!(&**dispatch, SupervisorControlCommandDispatch::ReloadConfig(_))
        ),
        _ => false,
    })
}

#[cfg(test)]
#[cfg(feature = "peios-registry")]
mod tests {
    use super::{CalendarTimerMaintenanceStep, calendar_timer_maintenance_steps};

    #[test]
    fn calendar_timer_reconfiguration_follows_ready_timer_processing_after_reload() {
        assert_eq!(
            calendar_timer_maintenance_steps(true),
            vec![
                CalendarTimerMaintenanceStep::ProcessReadyTimers,
                CalendarTimerMaintenanceStep::ReconfigureAfterReload,
            ],
        );
    }

    #[test]
    fn calendar_timer_processing_still_runs_without_reload_reconfiguration() {
        assert_eq!(
            calendar_timer_maintenance_steps(false),
            vec![CalendarTimerMaintenanceStep::ProcessReadyTimers],
        );
    }
}
