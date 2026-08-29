use crate::boundary::{
    ChildReaper, Clock, FilesystemCheckHelperLauncher, ProcessController, ProcessLauncher,
    RealtimeClock, ShutdownFinalizer, TokenProvider,
};
use crate::control::connection::{ControlConnectionIo, ControlListener};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::supervisor::Supervisor;

use super::model::{RuntimeShutdownLoopContext, RuntimeShutdownLoopError};
use crate::runtime::{
    RuntimeEventRegistrar, RuntimeEventSource, RuntimeLifecycleDeadlineTimer, RuntimeNotifySource,
    RuntimePid1SignalSource, RuntimeShutdownDeadlineTimer, RuntimeShutdownEventSources,
    RuntimeShutdownEventTurnError, RuntimeWorkPumpTurn, drain_runtime_work_queues,
    register_filesystem_check_helper_sources, register_process_setup_sources,
};

pub(crate) fn prepare_runtime_shutdown_loop_turn<I, L, S, H, N, D, M, C, P, F, A, R, T, K, B>(
    supervisor: &mut Supervisor,
    event_sources: &mut RuntimeShutdownEventSources<'_, I, L, S, H, N, D, M>,
    context: &mut RuntimeShutdownLoopContext<'_, C, P, F, A, R, T, K, B>,
) -> Result<RuntimeWorkPumpTurn, RuntimeShutdownLoopError>
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
    flush_jobs_waits(supervisor, event_sources.jobs_channel, context.clock)?;
    supervisor
        .sync_lifecycle_deadline_timer(event_sources.lifecycle_timer)
        .map_err(|error| RuntimeShutdownLoopError::Event {
            source: RuntimeEventSource::LifecycleDeadlineTimer,
            error: RuntimeShutdownEventTurnError::Supervisor(error),
        })?;
    Ok(pre_work)
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
