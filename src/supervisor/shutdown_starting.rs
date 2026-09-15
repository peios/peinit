use crate::boundary::{BoundaryError, ProcessController};
use crate::ids::JobId;
use crate::job::service_cgroup_root_path;
use crate::job::{JobEvent, JobState};
use crate::operation::store::OperationEvent;
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::shutdown::{ShutdownError, ShutdownPlan, ShutdownPostKillDeadline};

use super::dispatch::{SupervisorCancelledProcessSetupDispatch, SupervisorShutdownKillDispatch};
use super::process_setup::cleanup_pending_setup_process;
use super::state::SupervisorError;
use super::work::SupervisorWork;

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(super) fn kill_starting_services<P>(
    work: &mut SupervisorWork,
    plan: &ShutdownPlan,
    controller: &mut P,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<StartingServiceKillResult, ShutdownError>
where
    P: ProcessController + ?Sized,
{
    let mut kills = Vec::with_capacity(plan.starting_to_kill.len());
    let mut operation_events = Vec::new();
    let mut job_events = Vec::new();
    let mut cancelled_setups = Vec::new();
    let mut post_kill_deadlines = Vec::new();

    for service in &plan.starting_to_kill {
        if let Some(operation) = work.operations.current_for_service(service).cloned() {
            work.start.remove_pre_start_hook_deadline(operation.id);
            work.start.remove_pre_start_sequence(operation.id);
            work.start.remove_readiness_deadline(operation.id);
            operation_events.push(
                work.operations
                    .fail_operation(operation.id, now_ns, "startup killed by shutdown")
                    .map_err(ShutdownError::OperationStore)?,
            );
        }
        let cancelled = fail_unforked_startup_jobs(work, service, controller, now_ns)?;
        job_events.extend(cancelled.job_events);
        cancelled_setups.extend(cancelled.setups);
        let has_running_jobs = has_running_jobs(work, service)?;

        let cgroup_id = service_cgroup_id(&work.services, service)?;
        controller
            .kill_cgroup(&cgroup_id)
            .map_err(ShutdownError::Boundary)?;
        if has_running_jobs {
            post_kill_deadlines.push(ShutdownPostKillDeadline {
                service: service.clone(),
                cgroup_id: cgroup_id.clone(),
                due_at_ns: now_ns
                    .saturating_add(post_kill_timeout_secs.saturating_mul(NANOS_PER_SEC)),
            });
        }
        let service_transition = work
            .services
            .transition_service(
                service,
                ServiceTransition {
                    to: ServiceState::Failed,
                    cause: TransitionCause::ShutdownWave,
                },
            )
            .map_err(ShutdownError::ServiceTable)?;
        kills.push(SupervisorShutdownKillDispatch {
            service: service.clone(),
            cgroup_id,
            service_transition,
        });
    }

    Ok(StartingServiceKillResult {
        killed_starting: kills,
        operation_events,
        job_events,
        cancelled_setups,
        post_kill_deadlines,
    })
}

pub(super) struct StartingServiceKillResult {
    pub killed_starting: Vec<SupervisorShutdownKillDispatch>,
    pub operation_events: Vec<OperationEvent>,
    pub job_events: Vec<JobEvent>,
    pub cancelled_setups: Vec<SupervisorCancelledProcessSetupDispatch>,
    pub post_kill_deadlines: Vec<ShutdownPostKillDeadline>,
}

struct CancelledStartupJobs {
    job_events: Vec<JobEvent>,
    setups: Vec<SupervisorCancelledProcessSetupDispatch>,
}

fn service_cgroup_id(services: &ServiceTable, service: &str) -> Result<String, ShutdownError> {
    let runtime = services.runtime(service).ok_or_else(|| {
        ShutdownError::Plan(crate::shutdown::ShutdownPlanError::MissingRuntime {
            service: service.to_string(),
        })
    })?;
    Ok(service_cgroup_root_path(service, runtime.cgroup_generation))
}

/// Fail the service's jobs that have not started, whether or not they have
/// been forked.
///
/// A job stays `Created` from its launch until its setup status is read, and
/// in that window it owns a pending process setup whose descriptor is
/// registered with epoll. Failing the job removes its record, so the setup
/// must go with it: left behind, its next readiness reached
/// `process_pending_process_setup_status` with a job that no longer existed,
/// and that `UnknownJob` ended PID 1's runtime loop mid-shutdown (PEI-826).
fn fail_unforked_startup_jobs<P>(
    work: &mut SupervisorWork,
    service: &str,
    controller: &mut P,
    now_ns: u64,
) -> Result<CancelledStartupJobs, ShutdownError>
where
    P: ProcessController + ?Sized,
{
    let created_jobs = active_jobs_in_state(work, service, JobState::Created)?;
    remove_pending_jobs(work, &created_jobs);
    let setups = drop_pending_setups(work, service, &created_jobs, controller)?;

    let mut job_events = Vec::with_capacity(created_jobs.len());
    for job_id in created_jobs {
        job_events.push(
            work.jobs
                .fail_job_before_start(job_id, now_ns, "startup cancelled by shutdown")
                .map_err(ShutdownError::JobStore)?,
        );
    }
    Ok(CancelledStartupJobs { job_events, setups })
}

fn drop_pending_setups<P>(
    work: &mut SupervisorWork,
    service: &str,
    job_ids: &[JobId],
    controller: &mut P,
) -> Result<Vec<SupervisorCancelledProcessSetupDispatch>, ShutdownError>
where
    P: ProcessController + ?Sized,
{
    let setup_fds: Vec<i32> = work
        .pending_process_setups
        .iter()
        .filter(|(_, setup)| job_ids.contains(&setup.job_id))
        .map(|(fd, _)| *fd)
        .collect();
    let mut cancelled = Vec::with_capacity(setup_fds.len());
    for setup_status_fd in setup_fds {
        let Some(setup) = work.pending_process_setups.remove(&setup_status_fd) else {
            continue;
        };
        let cgroup_id = work
            .jobs
            .get(setup.job_id)
            .map(|job| job.cgroup_id.clone())
            .ok_or(ShutdownError::MissingJobRecord {
                job_id: setup.job_id,
            })?;
        cleanup_pending_setup_process(&setup.process, &cgroup_id, controller).map_err(|error| {
            match error {
                SupervisorError::ProcessControl(error) => ShutdownError::Boundary(error),
                other => ShutdownError::Boundary(BoundaryError::Process(format!(
                    "releasing the cancelled process setup of {service} failed: {other:?}"
                ))),
            }
        })?;
        cancelled.push(SupervisorCancelledProcessSetupDispatch {
            job_id: setup.job_id,
            service: service.to_string(),
            setup_status_fd,
        });
    }
    Ok(cancelled)
}

fn has_running_jobs(work: &SupervisorWork, service: &str) -> Result<bool, ShutdownError> {
    Ok(!active_jobs_in_state(work, service, JobState::Running)?.is_empty())
}

fn active_jobs_in_state(
    work: &SupervisorWork,
    service: &str,
    state: JobState,
) -> Result<Vec<JobId>, ShutdownError> {
    let mut matching = Vec::new();
    for job_id in work.jobs.active_for_service(service) {
        let job = work
            .jobs
            .get(job_id)
            .ok_or(ShutdownError::MissingJobRecord { job_id })?;
        if job.state == state {
            matching.push(job_id);
        }
    }
    Ok(matching)
}

fn remove_pending_jobs(work: &mut SupervisorWork, job_ids: &[JobId]) {
    work.pending_launches
        .retain(|job_id| !job_ids.contains(job_id));
    work.pending_start_hook_launches
        .retain(|job_id| !job_ids.contains(job_id));
    work.pending_post_hook_launches
        .retain(|job_id| !job_ids.contains(job_id));
    work.pending_control_launches
        .retain(|job_id| !job_ids.contains(job_id));
}
