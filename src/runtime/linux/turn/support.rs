use crate::boundary::{KmesEvent, KmesEventSink, ProcessController};
use crate::control::connection::{
    ControlConnectionIo, ControlConnectionRecord, ControlConnectionTable,
};
#[cfg(feature = "peios-registry")]
use crate::runtime::RuntimeRegistryWatchTurn;
use crate::runtime::{
    RuntimeCalendarTimerTurn, RuntimeShutdownEventTurn, RuntimeShutdownFinalizationTurn,
    RuntimeShutdownLoopError, RuntimeShutdownLoopTurn, RuntimeWorkPumpTurn,
    collect_runtime_loop_kmes_events, collect_runtime_shutdown_finalization_kmes_events,
    finalize_due_shutdown,
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

/// The final action the turn ends in, if one is due: a shutdown's, or the
/// reboot for a Critical service out of restart budget.
///
/// Once per turn, after the turn's events have been applied and its console
/// output written — the action does not return (PEI-827). The Critical
/// reboot is asked as one question about the service table rather than
/// raised by the path that observed the failure: enumerating those paths is
/// what was incomplete before (PEI-341). The deadline timer is re-synced
/// afterwards, as on every other path that changes the shutdown's
/// deadlines: a `reboot(2)` that returned leaves a retry to arm (PEI-1087).
pub(in crate::runtime::linux::turn) fn finalize_due_shutdown_at(
    supervisor: &mut Supervisor,
    finalizer: &mut dyn crate::boundary::ShutdownFinalizer,
    deadline_timer: &mut dyn crate::boundary::ShutdownDeadlineTimer,
    now_ns: u64,
) -> Result<Option<RuntimeShutdownFinalizationTurn>, RuntimeShutdownLoopError> {
    finalize_due_shutdown(supervisor, finalizer, deadline_timer, now_ns)
        .map_err(RuntimeShutdownLoopError::Finalization)
}

pub(in crate::runtime::linux::turn) fn flush_operation_waits_at<I>(
    supervisor: &Supervisor,
    control_connections: &mut ControlConnectionTable<ControlConnectionRecord<I>>,
    now_ns: u64,
    realtime_now_ns: u64,
) -> Result<(), RuntimeShutdownLoopError>
where
    I: ControlConnectionIo,
{
    supervisor
        .flush_terminal_control_waits(control_connections, now_ns, realtime_now_ns)
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

pub(in crate::runtime::linux::turn) fn prepend_idle_jobs_closure_turn(
    turn: &mut RuntimeShutdownLoopTurn,
    fds: Vec<i32>,
) {
    if !fds.is_empty() {
        turn.turns.insert(
            0,
            RuntimeShutdownEventTurn::IdleJobsConnectionsClosed { fds },
        );
    }
}

pub(in crate::runtime::linux::turn) fn append_idle_jobs_closure_turn(
    turn: &mut RuntimeShutdownLoopTurn,
    fds: Vec<i32>,
) {
    if !fds.is_empty() {
        turn.turns
            .push(RuntimeShutdownEventTurn::IdleJobsConnectionsClosed { fds });
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

/// An event the ring refused, dropped in its place (PEI-1082, PEI-1125).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DroppedKmesEvent {
    pub event_type: String,
    pub service: Option<String>,
    pub job_id: Option<String>,
    pub size_bytes: usize,
    pub error: String,
    /// How many events this boot has dropped, this one included.
    pub dropped_total: u64,
}

impl DroppedKmesEvent {
    fn oversized(&self) -> crate::kmes::OversizedEvent<'_> {
        crate::kmes::OversizedEvent {
            event_type: &self.event_type,
            action: crate::kmes::OversizedEventAction::Dropped,
            service: self.service.as_deref(),
            job_id: self.job_id.as_deref(),
            size_bytes: self.size_bytes as u64,
            limit_bytes: None,
            dropped_total: Some(self.dropped_total),
            error: Some(&self.error),
        }
    }

    pub(crate) fn console_message(&self) -> String {
        format!(
            "peinit warning: {}\n",
            crate::kmes::oversized_event_message(&self.oversized())
        )
    }
}

/// The ring and the count of what it has refused, for one emission.
pub(in crate::runtime::linux::turn) struct RuntimeKmesEmitter<'a> {
    pub sink: &'a mut dyn KmesEventSink,
    pub dropped_events: &'a mut u64,
}

pub(in crate::runtime::linux::turn) fn emit_runtime_loop_kmes_events(
    emitter: RuntimeKmesEmitter<'_>,
    pre_work: &RuntimeWorkPumpTurn,
    maintenance_before_wait: &SupervisorOperationMaintenanceTurn,
    event_turns: &[RuntimeShutdownEventTurn],
    post_work: &RuntimeWorkPumpTurn,
    maintenance_after_sources: &SupervisorOperationMaintenanceTurn,
    calendar_turns: &[(i32, RuntimeCalendarTimerTurn)],
) -> Result<Vec<DroppedKmesEvent>, RuntimeShutdownLoopError> {
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
    emit_kmes_events_contained(emitter, &events)
}

/// The audit record of a final action that returned.
pub(in crate::runtime::linux::turn) fn emit_runtime_shutdown_finalization_kmes_events(
    emitter: RuntimeKmesEmitter<'_>,
    finalization: &RuntimeShutdownFinalizationTurn,
) -> Result<Vec<DroppedKmesEvent>, RuntimeShutdownLoopError> {
    let mut events = Vec::new();
    collect_runtime_shutdown_finalization_kmes_events(finalization, &mut events)
        .map_err(RuntimeShutdownLoopError::Kmes)?;
    emit_kmes_events_contained(emitter, &events)
}

/// Emit the turn's events one at a time, dropping any the ring refuses.
///
/// A refusal of one event is that event's problem: too large for
/// `MaxEventSize`, most likely, which one job's `arguments` could arrange
/// from any authenticated principal (PEI-1082). Until PEI-1125 it was
/// treated as the ring being unusable, and PID 1 entered recovery over an
/// audit record it could not write. Now the event is dropped, counted, and
/// replaced in the trail by a small `event.oversized` naming it. That small
/// event fits by construction, so if the ring refuses *it* too the ring
/// really is unusable, and that is still the loop's error.
///
/// Emitted singly rather than as one batch because a batch stops at its
/// first refusal without saying which event it was, and re-sending the
/// batch to find out would duplicate everything before it.
fn emit_kmes_events_contained(
    emitter: RuntimeKmesEmitter<'_>,
    events: &[KmesEvent],
) -> Result<Vec<DroppedKmesEvent>, RuntimeShutdownLoopError> {
    let RuntimeKmesEmitter {
        sink,
        dropped_events,
    } = emitter;
    let mut dropped = Vec::new();
    for event in events {
        let Err(error) = sink.emit_kmes_event(event) else {
            continue;
        };
        *dropped_events += 1;
        let (service, job_id) = crate::kmes::kmes_event_subject(&event.payload);
        let record = DroppedKmesEvent {
            event_type: event.event_type.clone(),
            service,
            job_id,
            size_bytes: event.payload.len(),
            error: format!("{error:?}"),
            dropped_total: *dropped_events,
        };
        let notice = crate::kmes::encode_event_oversized_event(&record.oversized())
            .map_err(RuntimeShutdownLoopError::Kmes)?;
        sink.emit_kmes_event(&notice)
            .map_err(RuntimeShutdownLoopError::Kmes)?;
        dropped.push(record);
    }
    Ok(dropped)
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
        }
        | RuntimeShutdownEventTurn::DeferredRegistryReload {
            turn: crate::runtime::RuntimeDeferredRegistryReloadTurn { outcome, .. },
        } => outcome.is_ok(),
        RuntimeShutdownEventTurn::ControlConnection {
            supervisor: supervisor_turn,
            ..
        } => supervisor_turn.turn.frames.iter().any(|frame| {
            matches!(
                &frame.frame,
                SupervisorControlFrameTurn::CommandAccepted {
                    dispatch: Some(dispatch),
                    ..
                } if matches!(&**dispatch, SupervisorControlCommandDispatch::ReloadConfig(_))
            )
        }),
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
