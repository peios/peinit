mod model;
mod prepare;
mod sources;

pub use model::{
    DEFAULT_MAX_CONTROL_READ_BYTES, RuntimeControlLimits, RuntimeEventWaitError,
    RuntimeEventWaiter, RuntimeShutdownLoopContext, RuntimeShutdownLoopError,
    RuntimeShutdownLoopTurn,
};
pub(crate) use prepare::prepare_runtime_shutdown_loop_turn;
#[cfg(any(test, feature = "peios-boundary"))]
pub(crate) use sources::process_runtime_shutdown_sources_with_registry;

use crate::boundary::{
    ChildReaper, Clock, FilesystemCheckHelperLauncher, ProcessController, ProcessLauncher,
    RealtimeClock, ShutdownFinalizer, TokenProvider,
};
use crate::control::connection::{ControlConnectionIo, ControlListener};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::supervisor::Supervisor;

use super::{
    RuntimeEventRegistrar, RuntimeLifecycleDeadlineTimer, RuntimeNotifySource,
    RuntimePid1SignalSource, RuntimeShutdownDeadlineTimer, RuntimeShutdownEventSources,
};
use sources::process_runtime_shutdown_sources;

pub fn process_runtime_shutdown_loop_turn<I, L, S, H, N, D, M, W, C, P, F, A, R, T, K, B>(
    supervisor: &mut Supervisor,
    waiter: &mut W,
    event_sources: &mut RuntimeShutdownEventSources<'_, I, L, S, H, N, D, M>,
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
    W: RuntimeEventWaiter + ?Sized,
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
    let pre_work = prepare_runtime_shutdown_loop_turn(supervisor, event_sources, &mut context)?;
    let sources = waiter
        .wait_runtime_events(context.max_events)
        .map_err(RuntimeShutdownLoopError::Wait)?;
    let mut turn = process_runtime_shutdown_sources(supervisor, sources, event_sources, context)?;
    turn.pre_work = pre_work;
    Ok(turn)
}

#[cfg(test)]
mod tests;
