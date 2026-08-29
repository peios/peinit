use crate::boundary::{Clock, ProcessController, RealtimeClock, ShutdownFinalizer};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::jobs::socket::JobsSocketLimits;
use crate::runtime::{RuntimeJobsChannel, RuntimeJobsChannelError, RuntimeJobsContext};
use crate::supervisor::Supervisor;

use super::model::{
    RuntimeEventRegistrar, RuntimeShutdownEventContext, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError,
};

pub(super) fn process_jobs_listener_event<R>(
    jobs_channel: &mut dyn RuntimeJobsChannel,
    registrar: &mut R,
    accepted_at_ns: u64,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    R: RuntimeEventRegistrar + ?Sized,
{
    let mut registrar = registrar;
    let (accept, registration) = jobs_channel
        .accept_jobs_connection(&mut registrar, accepted_at_ns)
        .map_err(RuntimeShutdownEventTurnError::Jobs)?;
    Ok(RuntimeShutdownEventTurn::JobsListener {
        accept,
        registration,
    })
}

pub(super) fn process_jobs_connection_event<C, P, F, A, R>(
    supervisor: &mut Supervisor,
    fd: i32,
    jobs_channel: &mut dyn RuntimeJobsChannel,
    context: RuntimeShutdownEventContext<'_, C, P, F, A, R>,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: ShutdownFinalizer,
    A: SystemAccessChecker
        + ServiceAccessChecker
        + crate::submitted::JobAccessChecker
        + crate::submitted::JobDescriptorFactory
        + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
{
    let observed_at_ns = context
        .clock
        .monotonic_ns()
        .map_err(RuntimeShutdownEventTurnError::Clock)?;
    let limits: JobsSocketLimits = context.jobs_limits;
    // The loop's generics are `?Sized`; a `&mut` to each is the sized value
    // the channel's object-safe interface takes.
    let mut registrar = context.registrar;
    let mut security = context.access_checker;
    let mut controller = context.controller;
    let mut clock = context.clock;
    let turn = jobs_channel
        .process_jobs_connection(
            fd,
            supervisor,
            &mut registrar,
            RuntimeJobsContext {
                identity_provider: context.job_identity_provider,
                security: &mut security,
                controller: &mut controller,
                clock: &mut clock,
                limits,
                observed_at_ns,
            },
        )
        .map_err(RuntimeShutdownEventTurnError::Jobs)?;
    Ok(RuntimeShutdownEventTurn::JobsConnection { fd, turn })
}

impl From<RuntimeJobsChannelError> for RuntimeShutdownEventTurnError {
    fn from(error: RuntimeJobsChannelError) -> Self {
        Self::Jobs(error)
    }
}
