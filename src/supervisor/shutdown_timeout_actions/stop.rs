use crate::boundary::ProcessController;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::shutdown::{ShutdownError, ShutdownPostKillDeadline};

use crate::supervisor::dispatch::SupervisorShutdownCgroupKillDispatch;
use crate::supervisor::shutdown_progress::service_done_for_shutdown;
use crate::supervisor::shutdown_wave::service_root_cgroup_id;
use crate::supervisor::work::SupervisorWork;

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(in crate::supervisor) fn process_due_stop_deadlines<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<Vec<SupervisorShutdownCgroupKillDispatch>, ShutdownError>
where
    P: ProcessController + ?Sized,
{
    let shutdown = work
        .shutdown
        .as_mut()
        .ok_or(ShutdownError::NoShutdownInProgress)?;
    let (due, pending): (Vec<_>, Vec<_>) = shutdown
        .stop_deadlines
        .drain(..)
        .partition(|deadline| deadline.due_at_ns <= now_ns);
    shutdown.stop_deadlines = pending;

    let mut dispatches = Vec::with_capacity(due.len());
    for deadline in due {
        if let Some(operation_id) = deadline.operation_id {
            work.control.remove_stop_timeout(operation_id);
        }
        controller
            .kill_cgroup(&deadline.cgroup_id)
            .map_err(ShutdownError::Boundary)?;
        dispatches.push(SupervisorShutdownCgroupKillDispatch {
            service: deadline.service.clone(),
            cgroup_id: deadline.cgroup_id.clone(),
            killed_at_ns: now_ns,
        });
        record_post_kill_deadline(
            work,
            deadline.service,
            deadline.cgroup_id,
            now_ns,
            post_kill_timeout_secs,
        )?;
    }
    Ok(dispatches)
}

pub(in crate::supervisor) fn kill_all_remaining_services<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<Vec<SupervisorShutdownCgroupKillDispatch>, ShutdownError>
where
    P: ProcessController + ?Sized,
{
    let remaining = remaining_shutdown_services(work);
    let mut dispatches = Vec::with_capacity(remaining.len());
    if let Some(shutdown) = work.shutdown.as_mut() {
        shutdown.stop_deadlines.clear();
    }

    for service in remaining {
        let cgroup_id = service_root_cgroup_id(&work.services, &service)?;
        controller
            .kill_cgroup(&cgroup_id)
            .map_err(ShutdownError::Boundary)?;
        if matches!(
            work.services.runtime(&service).map(|runtime| runtime.state),
            Some(ServiceState::Active | ServiceState::Reloading)
        ) {
            work.services
                .transition_service(
                    &service,
                    ServiceTransition {
                        to: ServiceState::Stopping,
                        cause: TransitionCause::ShutdownWave,
                    },
                )
                .map_err(ShutdownError::ServiceTable)?;
        }
        dispatches.push(SupervisorShutdownCgroupKillDispatch {
            service: service.clone(),
            cgroup_id: cgroup_id.clone(),
            killed_at_ns: now_ns,
        });
        record_post_kill_deadline(work, service, cgroup_id, now_ns, post_kill_timeout_secs)?;
    }

    Ok(dispatches)
}

fn remaining_shutdown_services(work: &SupervisorWork) -> Vec<String> {
    let Some(shutdown) = &work.shutdown else {
        return Vec::new();
    };
    shutdown
        .plan
        .stop_waves
        .iter()
        .flat_map(|wave| wave.services.iter())
        .filter(|participant| {
            work.services
                .runtime(&participant.service)
                .is_some_and(|runtime| !service_done_for_shutdown(runtime.state))
        })
        .map(|participant| participant.service.clone())
        .collect()
}

fn record_post_kill_deadline(
    work: &mut SupervisorWork,
    service: String,
    cgroup_id: String,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<(), ShutdownError> {
    let shutdown = work
        .shutdown
        .as_mut()
        .ok_or(ShutdownError::NoShutdownInProgress)?;
    if shutdown
        .post_kill_deadlines
        .iter()
        .any(|deadline| deadline.service == service)
    {
        return Ok(());
    }
    shutdown.post_kill_deadlines.push(ShutdownPostKillDeadline {
        service,
        cgroup_id,
        due_at_ns: now_ns.saturating_add(post_kill_timeout_secs.saturating_mul(NANOS_PER_SEC)),
    });
    Ok(())
}
