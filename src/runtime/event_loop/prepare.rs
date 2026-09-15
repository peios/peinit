use crate::boundary::{
    ChildReaper, Clock, FilesystemCheckHelperLauncher, ProcessController, ProcessLauncher,
    RealtimeClock, RegistryClient, ShutdownFinalizer, TokenProvider,
};
use crate::control::connection::{
    ControlConnectionIo, ControlConnectionRecord, ControlConnectionTable, ControlListener,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::supervisor::Supervisor;

use super::model::{RuntimeShutdownLoopContext, RuntimeShutdownLoopError};
use crate::runtime::{
    RuntimeEventRegistrar, RuntimeEventSource, RuntimeLifecycleDeadlineTimer, RuntimeNotifySource,
    RuntimePid1SignalSource, RuntimeShutdownDeadlineTimer, RuntimeShutdownEventContext,
    RuntimeShutdownEventSources, RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError,
    RuntimeWorkPumpTurn, drain_runtime_work_queues, process_runtime_control_connection_event,
    register_filesystem_check_helper_sources, register_process_setup_sources,
    run_deferred_registry_reload,
};

/// Drive a turn for every control connection holding a complete frame that
/// no wait is holding back.
///
/// A request read in the same call as a `wait` command sits in the
/// connection's buffer until the wait clears, and no readable event will
/// ever arrive for bytes peinit has already read. So each place that answers
/// waits follows up here, and the buffered request is answered in the same
/// loop turn (PEI-1073).
pub(crate) fn resume_buffered_control_frames<I, D, C, P, F, A, R, G>(
    supervisor: &mut Supervisor,
    control_connections: &mut ControlConnectionTable<ControlConnectionRecord<I>>,
    mut registry: Option<&mut G>,
    deadline_timer: &mut D,
    mut context: RuntimeShutdownEventContext<'_, C, P, F, A, R>,
) -> Result<Vec<RuntimeShutdownEventTurn>, RuntimeShutdownLoopError>
where
    I: ControlConnectionIo,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: ShutdownFinalizer,
    A: SystemAccessChecker
        + ServiceAccessChecker
        + crate::submitted::JobAccessChecker
        + crate::submitted::JobDescriptorFactory
        + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
    G: RegistryClient,
{
    let mut turns = Vec::new();
    for fd in control_connections.fds_with_runnable_frames() {
        let turn = process_runtime_control_connection_event(
            supervisor,
            fd,
            control_connections,
            registry.as_deref_mut(),
            deadline_timer,
            context.reborrow(),
        )
        .map_err(|error| RuntimeShutdownLoopError::Event {
            source: RuntimeEventSource::ControlConnection { fd },
            error,
        })?;
        turns.push(turn);
    }
    Ok(turns)
}

/// The work drained before this turn waits, and the turns of any control
/// connection whose buffered request that work unblocked.
pub(crate) struct RuntimePreparedLoopTurn {
    pub pre_work: RuntimeWorkPumpTurn,
    pub resumed_control_turns: Vec<RuntimeShutdownEventTurn>,
}

pub(crate) fn prepare_runtime_shutdown_loop_turn<I, L, S, H, N, D, M, C, P, F, A, R, T, K, B, G>(
    supervisor: &mut Supervisor,
    event_sources: &mut RuntimeShutdownEventSources<'_, I, L, S, H, N, D, M>,
    control_registry: Option<&mut G>,
    context: &mut RuntimeShutdownLoopContext<'_, C, P, F, A, R, T, K, B>,
) -> Result<RuntimePreparedLoopTurn, RuntimeShutdownLoopError>
where
    I: ControlConnectionIo,
    L: ControlListener<Connection = I> + ?Sized,
    S: RuntimePid1SignalSource + ?Sized,
    H: ChildReaper + ?Sized,
    N: RuntimeNotifySource + ?Sized,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    M: RuntimeLifecycleDeadlineTimer + ?Sized,
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: ShutdownFinalizer,
    A: SystemAccessChecker
        + ServiceAccessChecker
        + crate::submitted::JobAccessChecker
        + crate::submitted::JobDescriptorFactory
        + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
    T: TokenProvider + ?Sized,
    K: ProcessLauncher,
    B: FilesystemCheckHelperLauncher + ?Sized,
    G: RegistryClient,
{
    let pre_work = drain_runtime_work_queues(supervisor, &mut context.work_pump_context())
        .map_err(RuntimeShutdownLoopError::Work)?;
    event_sources
        .log_pipes
        .register_work_pump_turn(&pre_work, context.registrar)
        .map_err(RuntimeShutdownLoopError::LogRegistration)?;
    register_filesystem_check_helper_sources(&pre_work, context.registrar)
        .map_err(RuntimeShutdownLoopError::EventRegistration)?;
    register_process_setup_sources(&pre_work, context.registrar)
        .map_err(RuntimeShutdownLoopError::EventRegistration)?;
    let (wait_flush_observed_at_ns, wait_flush_realtime_ns) =
        if event_sources.control_connections.has_pending_waits() {
            (
                context
                    .clock
                    .monotonic_ns()
                    .map_err(RuntimeShutdownLoopError::Clock)?,
                context
                    .clock
                    .realtime_ns()
                    .map_err(RuntimeShutdownLoopError::Clock)?,
            )
        } else {
            (0, 0)
        };
    supervisor
        .flush_terminal_control_waits(
            event_sources.control_connections,
            wait_flush_observed_at_ns,
            wait_flush_realtime_ns,
        )
        .map_err(RuntimeShutdownLoopError::ControlWait)?;
    let mut control_registry = control_registry;
    // The pre-wait pump may have drained the boot plan (a launch that
    // failed, say); the reload the boot window deferred runs here rather
    // than after a wait that could be long (PEI-350).
    let mut resumed_control_turns =
        run_deferred_registry_reload(supervisor, control_registry.as_deref_mut())
            .into_iter()
            .collect::<Vec<_>>();
    resumed_control_turns.extend(resume_buffered_control_frames(
        supervisor,
        event_sources.control_connections,
        control_registry,
        event_sources.deadline_timer,
        context.event_context(),
    )?);
    flush_jobs_waits(supervisor, event_sources.jobs_channel, context.clock)?;
    supervisor
        .sync_lifecycle_deadline_timer(event_sources.lifecycle_timer)
        .map_err(|error| RuntimeShutdownLoopError::Event {
            source: RuntimeEventSource::LifecycleDeadlineTimer,
            error: RuntimeShutdownEventTurnError::Supervisor(error),
        })?;
    Ok(RuntimePreparedLoopTurn {
        pre_work,
        resumed_control_turns,
    })
}

/// Answer every jobs-channel wait whose condition now holds.
pub(super) fn flush_jobs_waits<C>(
    supervisor: &Supervisor,
    jobs_channel: &mut dyn crate::runtime::RuntimeJobsChannel,
    clock: &mut C,
) -> Result<(), RuntimeShutdownLoopError>
where
    C: Clock + RealtimeClock + ?Sized,
{
    if !jobs_channel.has_pending_jobs_waits() {
        return Ok(());
    }
    let monotonic_now_ns = clock
        .monotonic_ns()
        .map_err(RuntimeShutdownLoopError::Clock)?;
    let realtime_now_ns = clock
        .realtime_ns()
        .map_err(RuntimeShutdownLoopError::Clock)?;
    jobs_channel
        .flush_jobs_waits(
            supervisor,
            crate::control::wire::ControlResponseTimeProjection::new(
                monotonic_now_ns,
                realtime_now_ns,
            ),
            monotonic_now_ns,
        )
        .map(|_| ())
        .map_err(RuntimeShutdownLoopError::JobsWait)
}
