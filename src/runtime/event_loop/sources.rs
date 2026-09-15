use crate::boundary::{
    ChildReaper, Clock, FilesystemCheckHelperLauncher, ProcessController, ProcessLauncher,
    RealtimeClock, RegistryClient, RegistryWatchSource, ShutdownFinalizer, TokenProvider,
};
use crate::control::connection::{ControlConnectionIo, ControlListener};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::service::runtime::ServiceState;
use crate::supervisor::Supervisor;

use super::model::{RuntimeShutdownLoopContext, RuntimeShutdownLoopError, RuntimeShutdownLoopTurn};
use crate::runtime::{
    NoRuntimeRegistryClient, RuntimeEventRegistrar, RuntimeEventSource,
    RuntimeLifecycleDeadlineTimer, RuntimeNotifySource, RuntimePid1SignalSource,
    RuntimeShutdownDeadlineTimer, RuntimeShutdownEventSources, RuntimeWorkPumpTurn,
    drain_runtime_work_queues, process_registry_watch_event,
    process_runtime_control_connection_event, process_runtime_shutdown_event,
    register_filesystem_check_helper_sources, register_process_setup_sources,
};

pub(super) fn process_runtime_shutdown_sources<I, L, S, H, N, D, M, C, P, F, A, R, T, K, B>(
    supervisor: &mut Supervisor,
    sources: Vec<RuntimeEventSource>,
    event_sources: &mut RuntimeShutdownEventSources<'_, I, L, S, H, N, D, M>,
    context: RuntimeShutdownLoopContext<'_, C, P, F, A, R, T, K, B>,
) -> Result<RuntimeShutdownLoopTurn, RuntimeShutdownLoopError>
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
    process_runtime_shutdown_sources_with_registry(
        supervisor,
        sources,
        event_sources,
        None::<&mut NoRuntimeRegistryClient>,
        None,
        context,
    )
}

pub(crate) fn process_runtime_shutdown_sources_with_registry<
    I,
    L,
    S,
    H,
    N,
    D,
    M,
    C,
    P,
    F,
    A,
    R,
    T,
    K,
    B,
    G,
>(
    supervisor: &mut Supervisor,
    sources: Vec<RuntimeEventSource>,
    event_sources: &mut RuntimeShutdownEventSources<'_, I, L, S, H, N, D, M>,
    mut control_registry: Option<&mut G>,
    mut registry_watch: Option<&mut dyn RegistryWatchSource>,
    mut context: RuntimeShutdownLoopContext<'_, C, P, F, A, R, T, K, B>,
) -> Result<RuntimeShutdownLoopTurn, RuntimeShutdownLoopError>
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
    let sources = prioritize_runtime_sources(sources);
    let mut turns = Vec::with_capacity(sources.len());
    for source in sources.iter().copied() {
        let turn = match source {
            RuntimeEventSource::ControlConnection { fd } => {
                process_runtime_control_connection_event(
                    supervisor,
                    fd,
                    &mut *event_sources.control_connections,
                    control_registry.as_deref_mut(),
                    &mut *event_sources.deadline_timer,
                    context.event_context(),
                )
            }
            RuntimeEventSource::RegistryWatch { fd } => Ok(process_registry_watch_event(
                supervisor,
                fd,
                control_registry.as_deref_mut(),
                registry_watch.as_deref_mut(),
                context.registrar,
            )),
            _ => process_runtime_shutdown_event(
                supervisor,
                source,
                event_sources,
                context.event_context(),
            ),
        }
        .map_err(|error| RuntimeShutdownLoopError::Event { source, error })?;
        turns.push(turn);
    }
    let post_work = drain_runtime_work_queues(supervisor, &mut context.work_pump_context())
        .map_err(RuntimeShutdownLoopError::Work)?;
    event_sources
        .log_pipes
        .register_work_pump_turn(&post_work, context.registrar)
        .map_err(RuntimeShutdownLoopError::LogRegistration)?;
    register_filesystem_check_helper_sources(&post_work, context.registrar)
        .map_err(RuntimeShutdownLoopError::EventRegistration)?;
    register_process_setup_sources(&post_work, context.registrar)
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
    turns.extend(super::prepare::resume_buffered_control_frames(
        supervisor,
        event_sources.control_connections,
        control_registry,
        event_sources.deadline_timer,
        context.event_context(),
    )?);
    super::prepare::flush_jobs_waits(supervisor, event_sources.jobs_channel, context.clock)?;
    let eventd_flush = event_sources.log_pipes.sync_eventd_forwarding(
        eventd_active(supervisor),
        supervisor.eventd_log_socket_path(),
    );

    Ok(RuntimeShutdownLoopTurn {
        pre_work: RuntimeWorkPumpTurn::default(),
        sources,
        turns,
        post_work,
        eventd_flush,
        finalization: None,
    })
}

fn prioritize_runtime_sources(sources: Vec<RuntimeEventSource>) -> Vec<RuntimeEventSource> {
    let mut indexed = sources.into_iter().enumerate().collect::<Vec<_>>();
    indexed.sort_by_key(|(index, source)| (runtime_source_priority(*source), *index));
    indexed.into_iter().map(|(_, source)| source).collect()
}

fn runtime_source_priority(source: RuntimeEventSource) -> u8 {
    match source {
        RuntimeEventSource::Pid1Signal | RuntimeEventSource::PowerButton { .. } => 0,
        RuntimeEventSource::ShutdownDeadlineTimer => 1,
        _ => 2,
    }
}

fn eventd_active(supervisor: &Supervisor) -> bool {
    supervisor
        .services()
        .runtime("eventd")
        .is_some_and(|runtime| runtime.state == ServiceState::Active)
}

#[cfg(test)]
mod tests {
    use crate::runtime::RuntimeEventSource;

    use super::prioritize_runtime_sources;

    #[test]
    fn source_priority_keeps_signals_ahead_of_log_pipe_load() {
        let sources = vec![
            RuntimeEventSource::ServiceLogPipe { fd: 10 },
            RuntimeEventSource::ControlConnection { fd: 11 },
            RuntimeEventSource::ShutdownDeadlineTimer,
            RuntimeEventSource::ServiceLogPipe { fd: 12 },
            RuntimeEventSource::Pid1Signal,
            RuntimeEventSource::PowerButton { fd: 13 },
            RuntimeEventSource::NotifySocket,
        ];

        assert_eq!(
            prioritize_runtime_sources(sources),
            vec![
                RuntimeEventSource::Pid1Signal,
                RuntimeEventSource::PowerButton { fd: 13 },
                RuntimeEventSource::ShutdownDeadlineTimer,
                RuntimeEventSource::ServiceLogPipe { fd: 10 },
                RuntimeEventSource::ControlConnection { fd: 11 },
                RuntimeEventSource::ServiceLogPipe { fd: 12 },
                RuntimeEventSource::NotifySocket,
            ],
        );
    }
}
