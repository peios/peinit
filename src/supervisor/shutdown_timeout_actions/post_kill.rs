use crate::boundary::ProcessController;
use crate::ids::JobId;
use crate::job::{JobEvent, JobState};
use crate::service::runtime::{LeakedCgroupKind, ServiceState, ServiceTransition, TransitionCause};
use crate::shutdown::ShutdownError;

use crate::supervisor::cgroup_cleanup::cleanup_service_cgroup_tree;
use crate::supervisor::dispatch::SupervisorShutdownAbandonedDispatch;
use crate::supervisor::work::SupervisorWork;

pub(in crate::supervisor) fn process_due_post_kill_deadlines<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    now_ns: u64,
) -> Result<PostKillDeadlineDispatch, ShutdownError>
where
    P: ProcessController + ?Sized,
{
    let shutdown = work
        .shutdown
        .as_mut()
        .ok_or(ShutdownError::NoShutdownInProgress)?;
    let (due, pending): (Vec<_>, Vec<_>) = shutdown
        .post_kill_deadlines
        .drain(..)
        .partition(|deadline| deadline.due_at_ns <= now_ns);
    shutdown.post_kill_deadlines = pending;

    let mut abandoned = Vec::new();
    let mut job_events = Vec::new();
    let mut processed = 0usize;
    for deadline in due {
        processed += 1;
        let state = work
            .services
            .runtime(&deadline.service)
            .map(|runtime| runtime.state);
        if controller
            .cgroup_populated(&deadline.cgroup_id)
            .map_err(ShutdownError::Boundary)?
        {
            let abandoned_job_events =
                abandon_running_jobs(work, &deadline.service, now_ns, "process survived SIGKILL")?;
            work.services
                .record_leaked_cgroup(
                    &deadline.service,
                    deadline.cgroup_id.clone(),
                    LeakedCgroupKind::ServiceTree,
                    now_ns,
                )
                .map_err(ShutdownError::ServiceTable)?;
            job_events.extend(abandoned_job_events);
            if state == Some(ServiceState::Abandoned) {
                continue;
            }
            let service_transition = work
                .services
                .transition_service(
                    &deadline.service,
                    ServiceTransition {
                        to: ServiceState::Abandoned,
                        cause: TransitionCause::ProcessUnkillable,
                    },
                )
                .map_err(ShutdownError::ServiceTable)?;
            abandoned.push(SupervisorShutdownAbandonedDispatch {
                service: deadline.service,
                cgroup_id: deadline.cgroup_id,
                service_transition,
            });
        } else {
            job_events.extend(fail_running_jobs(
                work,
                &deadline.service,
                now_ns,
                "service cgroup emptied after SIGKILL before exit status was observed",
            )?);
            cleanup_service_cgroup_tree(controller, &deadline.cgroup_id)
                .map_err(ShutdownError::Boundary)?;
            if state == Some(ServiceState::Stopping) {
                let cause = work
                    .services
                    .runtime(&deadline.service)
                    .and_then(|runtime| runtime.cause);
                let service_transition = work
                    .services
                    .transition_service(
                        &deadline.service,
                        ServiceTransition {
                            to: stopped_state(cause),
                            cause: stopped_cause(cause),
                        },
                    )
                    .map_err(ShutdownError::ServiceTable)?;
                let _ = service_transition;
            }
        }
    }
    Ok(PostKillDeadlineDispatch {
        abandoned,
        job_events,
        processed,
    })
}

pub(in crate::supervisor) struct PostKillDeadlineDispatch {
    pub abandoned: Vec<SupervisorShutdownAbandonedDispatch>,
    pub job_events: Vec<JobEvent>,
    pub processed: usize,
}

fn stopped_state(cause: Option<TransitionCause>) -> ServiceState {
    match cause {
        Some(TransitionCause::ConflictEviction | TransitionCause::BindsToPropagation) => {
            ServiceState::Failed
        }
        _ => ServiceState::Inactive,
    }
}

fn stopped_cause(cause: Option<TransitionCause>) -> TransitionCause {
    match cause {
        Some(
            cause @ (TransitionCause::ConflictEviction
            | TransitionCause::BindsToPropagation
            | TransitionCause::ShutdownWave),
        ) => cause,
        _ => TransitionCause::ExplicitStop,
    }
}

fn abandon_running_jobs(
    work: &mut SupervisorWork,
    service: &str,
    now_ns: u64,
    failure_cause: &'static str,
) -> Result<Vec<JobEvent>, ShutdownError> {
    let job_ids = active_jobs_in_state(work, service, JobState::Running)?;
    let mut events = Vec::with_capacity(job_ids.len());
    for job_id in job_ids {
        events.push(
            work.jobs
                .abandon_job(job_id, now_ns, failure_cause)
                .map_err(ShutdownError::JobStore)?,
        );
    }
    Ok(events)
}

fn fail_running_jobs(
    work: &mut SupervisorWork,
    service: &str,
    now_ns: u64,
    failure_cause: &'static str,
) -> Result<Vec<JobEvent>, ShutdownError> {
    let job_ids = active_jobs_in_state(work, service, JobState::Running)?;
    let mut events = Vec::with_capacity(job_ids.len());
    for job_id in job_ids {
        events.push(
            work.jobs
                .fail_running_job(job_id, now_ns, None, failure_cause)
                .map_err(ShutdownError::JobStore)?,
        );
    }
    Ok(events)
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
