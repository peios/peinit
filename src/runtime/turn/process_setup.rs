use crate::boundary::{Clock, ProcessController, ProcessLauncher, ProcessSetupStatus};
use crate::runtime::{
    RuntimeEventRegistrar, RuntimeEventRegistrationError, RuntimeEventSource,
    RuntimeProcessSetupTurn, RuntimeServiceLogPipes, RuntimeShutdownEventTurn,
};
use crate::supervisor::{
    Supervisor, SupervisorCancelledProcessSetupDispatch, SupervisorProcessSetupDispatch,
};

use super::model::RuntimeShutdownEventTurnError;

pub(super) fn process_process_setup_event<C, P, L, R>(
    supervisor: &mut Supervisor,
    fd: i32,
    launcher: &mut L,
    clock: &mut C,
    controller: &mut P,
    registrar: &mut R,
    log_pipes: &mut RuntimeServiceLogPipes,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
    L: ProcessLauncher + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
{
    if !supervisor.has_pending_process_setup(fd) {
        // The readiness is for a setup the supervisor no longer holds: a
        // shutdown earlier in this same turn cancelled the job and closed
        // the descriptor with it (PEI-826), and the event was already in
        // the batch epoll returned. Reading it now fails with EBADF -- or
        // reads whatever the number has since been reused for -- and the
        // failure ended the runtime loop. Nothing to read, nothing to
        // close: just make sure the registration is gone.
        let _ = registrar.unregister_source(fd);
        return Ok(RuntimeShutdownEventTurn::ProcessSetup {
            fd,
            turn: RuntimeProcessSetupTurn::Stale { fd },
        });
    }
    let status = launcher.read_process_setup_status(fd).map_err(|error| {
        RuntimeShutdownEventTurnError::Supervisor(crate::supervisor::SupervisorError::Launch(
            crate::execution::launch::LaunchCreatedJobError::Boundary(error),
        ))
    })?;
    let terminal = !matches!(status, ProcessSetupStatus::Pending);
    if terminal {
        registrar
            .unregister_source(fd)
            .map_err(RuntimeShutdownEventTurnError::EventRegistration)?;
    }
    let observed_at_ns = clock.monotonic_ns().map_err(|error| {
        RuntimeShutdownEventTurnError::Supervisor(crate::supervisor::SupervisorError::Clock(error))
    })?;
    let dispatch = supervisor
        .process_pending_process_setup_status(fd, status, observed_at_ns, controller)
        .map_err(RuntimeShutdownEventTurnError::Supervisor)?;
    let stale = matches!(dispatch, SupervisorProcessSetupDispatch::Stale { .. });
    if stale && !terminal {
        registrar
            .unregister_source(fd)
            .map_err(RuntimeShutdownEventTurnError::EventRegistration)?;
    }
    if terminal || stale {
        close_fd(fd);
    }
    let turn = match dispatch {
        SupervisorProcessSetupDispatch::Pending(pending) => {
            RuntimeProcessSetupTurn::Pending { pending }
        }
        SupervisorProcessSetupDispatch::Stale { setup_status_fd } => {
            RuntimeProcessSetupTurn::Stale {
                fd: setup_status_fd,
            }
        }
        dispatch => {
            let log_registrations =
                register_setup_completion_log_pipes(&dispatch, log_pipes, registrar)
                    .map_err(RuntimeShutdownEventTurnError::EventRegistration)?;
            RuntimeProcessSetupTurn::Completed {
                supervisor: Box::new(dispatch),
                log_registrations,
            }
        }
    };
    Ok(RuntimeShutdownEventTurn::ProcessSetup { fd, turn })
}

pub(crate) fn register_process_setup_sources<R>(
    turn: &crate::runtime::RuntimeWorkPumpTurn,
    registrar: &mut R,
) -> Result<Vec<RuntimeEventSource>, RuntimeEventRegistrationError>
where
    R: RuntimeEventRegistrar + ?Sized,
{
    let mut registrations = Vec::new();
    for pending in &turn.pending_process_setups {
        let source = RuntimeEventSource::ProcessSetup {
            fd: pending.setup_status_fd,
        };
        registrar.register_source(pending.setup_status_fd, source)?;
        registrations.push(source);
    }
    Ok(registrations)
}

/// Take the setups a shutdown cancelled out of epoll and close them.
///
/// The supervisor dropped these with their jobs (PEI-826); their descriptors
/// are still registered, and a readiness on one would be read against a job
/// that no longer exists. Removal is best effort, as on the other removal
/// paths: a descriptor that was never registered still has to be closed.
pub(super) fn release_cancelled_process_setups<R>(
    cancelled: &[SupervisorCancelledProcessSetupDispatch],
    registrar: &mut R,
) where
    R: RuntimeEventRegistrar + ?Sized,
{
    for setup in cancelled {
        let _ = registrar.unregister_source(setup.setup_status_fd);
        close_fd(setup.setup_status_fd);
    }
}

fn register_setup_completion_log_pipes<R>(
    dispatch: &SupervisorProcessSetupDispatch,
    log_pipes: &mut RuntimeServiceLogPipes,
    registrar: &mut R,
) -> Result<Vec<RuntimeEventSource>, RuntimeEventRegistrationError>
where
    R: RuntimeEventRegistrar + ?Sized,
{
    match dispatch {
        SupervisorProcessSetupDispatch::ServiceMainLaunched(dispatch) => {
            log_pipes.register_completed_launch(&dispatch.launch, registrar)
        }
        SupervisorProcessSetupDispatch::StartHookLaunched(dispatch) => {
            log_pipes.register_completed_launch(&dispatch.launch, registrar)
        }
        SupervisorProcessSetupDispatch::PostHookLaunched(dispatch) => {
            log_pipes.register_completed_launch(&dispatch.launch, registrar)
        }
        SupervisorProcessSetupDispatch::ControlLaunched(dispatch) => {
            log_pipes.register_completed_launch(&dispatch.launch, registrar)
        }
        SupervisorProcessSetupDispatch::HealthCheckLaunched(dispatch) => {
            log_pipes.register_completed_launch(&dispatch.launch, registrar)
        }
        SupervisorProcessSetupDispatch::SubmittedLaunched(dispatch) => {
            let mut registrations = Vec::new();
            log_pipes.register_submitted_launch(dispatch, registrar, &mut registrations)?;
            Ok(registrations)
        }
        _ => Ok(Vec::new()),
    }
}

fn close_fd(fd: i32) {
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
    }
}
