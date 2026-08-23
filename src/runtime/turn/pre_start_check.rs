use crate::boundary::{
    Clock, FilesystemCheckHelperReader, FilesystemCheckReport, FilesystemCheckResult,
    LaunchedFilesystemCheckHelper,
};
use crate::runtime::RuntimeEventSource;
use crate::supervisor::{Supervisor, SupervisorError};

use super::model::{
    RuntimeEventRegistrar, RuntimeFilesystemCheckHelperTurn, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError,
};

pub(super) fn process_filesystem_check_helper_event<C, R, H>(
    supervisor: &mut Supervisor,
    result_fd: i32,
    reader: &mut H,
    clock: &mut C,
    registrar: &mut R,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    C: Clock + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
    H: FilesystemCheckHelperReader + ?Sized,
{
    let Some(helper) = supervisor.filesystem_check_helper_by_result_fd(result_fd) else {
        // No unregister here. A stale event means the helper already completed
        // through one of the terminal paths below, which unregistered and then
        // closed both descriptors -- so the number may already have been reused
        // by an unrelated source, and unregistering it would drop that source
        // out of the event set.
        return Ok(RuntimeShutdownEventTurn::FilesystemCheckHelper {
            result_fd,
            turn: RuntimeFilesystemCheckHelperTurn::Stale { fd: result_fd },
        });
    };

    let report = match reader.read_filesystem_check_report(&helper) {
        Ok(Some(report)) => report,
        Ok(None) => {
            return Ok(RuntimeShutdownEventTurn::FilesystemCheckHelper {
                result_fd,
                turn: RuntimeFilesystemCheckHelperTurn::WouldBlock { result_fd },
            });
        }
        Err(error) => {
            let completion = complete_filesystem_check_with_report(
                supervisor,
                result_fd,
                failed_report(&helper),
                clock,
            )?;
            release_helper_sources(&helper, registrar, reader)?;
            return Ok(RuntimeShutdownEventTurn::FilesystemCheckHelper {
                result_fd,
                turn: RuntimeFilesystemCheckHelperTurn::ReadFailedClosed {
                    result_fd,
                    error,
                    completion: Box::new(completion),
                },
            });
        }
    };

    let completion = complete_filesystem_check_with_report(supervisor, result_fd, report, clock)?;
    release_helper_sources(&helper, registrar, reader)?;
    Ok(RuntimeShutdownEventTurn::FilesystemCheckHelper {
        result_fd,
        turn: RuntimeFilesystemCheckHelperTurn::Completed {
            completion: Box::new(completion),
        },
    })
}

pub(super) fn process_filesystem_check_helper_exit_event<C, R, H>(
    supervisor: &mut Supervisor,
    pidfd: i32,
    reader: &mut H,
    clock: &mut C,
    registrar: &mut R,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    C: Clock + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
    H: FilesystemCheckHelperReader + ?Sized,
{
    let Some(helper) = supervisor.filesystem_check_helper_by_pidfd(pidfd) else {
        // See `process_filesystem_check_helper_event`: a stale descriptor has
        // already been closed, so it must not be unregistered.
        return Ok(RuntimeShutdownEventTurn::FilesystemCheckHelperExit {
            pidfd,
            turn: RuntimeFilesystemCheckHelperTurn::Stale { fd: pidfd },
        });
    };

    let (report, failure) = match reader.read_filesystem_check_report(&helper) {
        Ok(Some(report)) => (report, None),
        Ok(None) => (
            failed_report(&helper),
            Some(crate::boundary::BoundaryError::Process(format!(
                "filesystem check helper pidfd {pidfd} exited without report",
            ))),
        ),
        Err(error) => (failed_report(&helper), Some(error)),
    };
    let completion =
        complete_filesystem_check_with_report(supervisor, helper.result_fd, report, clock)?;
    release_helper_sources(&helper, registrar, reader)?;
    let turn = match failure {
        Some(error) => RuntimeFilesystemCheckHelperTurn::ReadFailedClosed {
            result_fd: helper.result_fd,
            error,
            completion: Box::new(completion),
        },
        None => RuntimeFilesystemCheckHelperTurn::Completed {
            completion: Box::new(completion),
        },
    };

    Ok(RuntimeShutdownEventTurn::FilesystemCheckHelperExit { pidfd, turn })
}

fn complete_filesystem_check_with_report<C>(
    supervisor: &mut Supervisor,
    result_fd: i32,
    report: FilesystemCheckReport,
    clock: &mut C,
) -> Result<
    crate::supervisor::SupervisorFilesystemCheckCompletionDispatch,
    RuntimeShutdownEventTurnError,
>
where
    C: Clock + ?Sized,
{
    let observed_at_ns = clock.monotonic_ns().map_err(|error| {
        RuntimeShutdownEventTurnError::Supervisor(SupervisorError::Clock(error))
    })?;
    supervisor
        .complete_filesystem_check_helper(result_fd, report, observed_at_ns)
        .map_err(RuntimeShutdownEventTurnError::Supervisor)
}

/// Retire a finished helper: unregister both descriptors, then close them.
///
/// Both steps run for both descriptors even if one fails, so a registrar error
/// on the result descriptor cannot strand the pidfd. The close is unconditional
/// for the same reason -- an unregister failure is not a reason to leak a
/// descriptor in PID 1.
fn release_helper_sources<R, H>(
    helper: &LaunchedFilesystemCheckHelper,
    registrar: &mut R,
    reader: &mut H,
) -> Result<(), RuntimeShutdownEventTurnError>
where
    R: RuntimeEventRegistrar + ?Sized,
    H: FilesystemCheckHelperReader + ?Sized,
{
    release_helper_fds(helper.result_fd, helper.pidfd, registrar, reader)
}

pub(super) fn release_helper_fds<R, H>(
    result_fd: i32,
    pidfd: i32,
    registrar: &mut R,
    reader: &mut H,
) -> Result<(), RuntimeShutdownEventTurnError>
where
    R: RuntimeEventRegistrar + ?Sized,
    H: FilesystemCheckHelperReader + ?Sized,
{
    let result = registrar.unregister_source(result_fd);
    let exit = registrar.unregister_source(pidfd);
    reader.release_filesystem_check_helper_fds(result_fd, pidfd);
    result
        .and(exit)
        .map_err(RuntimeShutdownEventTurnError::EventRegistration)
}

fn failed_report(helper: &LaunchedFilesystemCheckHelper) -> FilesystemCheckReport {
    FilesystemCheckReport {
        service: helper.service.clone(),
        operation_id: helper.operation_id,
        results: helper
            .checks
            .iter()
            .cloned()
            .map(|check| FilesystemCheckResult {
                check,
                satisfied: false,
            })
            .collect(),
    }
}

pub(crate) fn register_filesystem_check_helper_sources<R>(
    turn: &crate::runtime::RuntimeWorkPumpTurn,
    registrar: &mut R,
) -> Result<Vec<RuntimeEventSource>, crate::runtime::RuntimeEventRegistrationError>
where
    R: RuntimeEventRegistrar + ?Sized,
{
    let mut registrations = Vec::new();
    for dispatch in &turn.filesystem_check_launches {
        let result_fd = dispatch.helper.result_fd;
        let result_source = RuntimeEventSource::FilesystemCheckHelper { result_fd };
        registrar.register_source(result_fd, result_source)?;
        registrations.push(result_source);

        let pidfd = dispatch.helper.pidfd;
        let pid_source = RuntimeEventSource::FilesystemCheckHelperExit { pidfd };
        if let Err(error) = registrar.register_source(pidfd, pid_source) {
            let _ = registrar.unregister_source(result_fd);
            return Err(error);
        }
        registrations.push(pid_source);
    }
    Ok(registrations)
}
