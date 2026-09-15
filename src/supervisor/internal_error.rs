//! Containing an internal error to the one service it happened on.
//!
//! Every path the runtime loop drives is either about one service — a job
//! terminal, a setup status, a health probe, a notify datagram — or about
//! supervision itself: the wait, the event ring, the listeners. An error on
//! the first kind used to be treated like an error on the second: it escaped
//! the runtime loop, PID 1 entered recovery and unlinked its sockets, and a
//! machine that was otherwise up became unadministrable over one service's
//! bookkeeping (PEI-824, PEI-803, PEI-826, PEI-1082).
//!
//! The rule now (PEI-1125): an internal error on a path attributable to one
//! service fails that service, is announced, and the loop carries on. This
//! module is the mechanism the runtime uses to do that. It is deliberately
//! infallible — its whole purpose is to be the thing that does not raise —
//! and best-effort: whatever bookkeeping cannot be settled is left as it is
//! and named in the evidence.

use crate::boundary::ProcessController;
use crate::ids::JobId;
use crate::job::{JobEvent, JobState};
use crate::operation::store::OperationEvent;
use crate::operation::{OperationState, internal_error_result};
use crate::service::ServiceTableTransition;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use super::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use super::health::apply_health_scheduling_after_transitions;
use super::relationships::apply_relationship_reactions_after_transitions;
use super::state::Supervisor;
use super::watchdog::apply_watchdog_scheduling_after_transitions;
use super::work::SupervisorWork;

/// What a per-service path was acting on when it raised.
///
/// Resolved *before* the supervisor call that may fail, because once the
/// call has returned an error the state it was about to touch is no longer
/// the way to find out.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct SupervisorInternalErrorSubject {
    pub service: Option<String>,
    pub job_id: Option<JobId>,
}

impl SupervisorInternalErrorSubject {
    pub fn service(service: impl Into<String>) -> Self {
        Self {
            service: Some(service.into()),
            job_id: None,
        }
    }

    pub fn is_attributable(&self) -> bool {
        self.service.is_some() || self.job_id.is_some()
    }
}

/// The evidence of one contained internal error: what was being done, what
/// went wrong, and what was failed because of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorInternalErrorDispatch {
    /// The step that raised, in the words the console uses: `job terminal`,
    /// `process setup`, `lifecycle deadline`, `notify`.
    pub step: &'static str,
    pub subject: SupervisorInternalErrorSubject,
    /// The error, rendered. Kept as text because the error types the paths
    /// raise are not one type, and the evidence needs to outlive them.
    pub error: String,
    pub observed_at_ns: u64,
    /// The job the subject was retired with, if it still had one.
    pub job_event: Option<JobEvent>,
    /// The service's main job, retired too when the subject job was another
    /// of the service's jobs — a health probe, a hook. A Failed service does
    /// not keep a running main process.
    pub service_job_event: Option<JobEvent>,
    /// The operation on the service, failed with the `internal_error` result
    /// so a waiting client is answered (§8.2, §10.1).
    pub operation_event: Option<OperationEvent>,
    /// `-> Failed` under `InternalError`, if the service was in a state that
    /// can fail. A service already Failed, Inactive or Completed keeps its
    /// state and only the announcement records the error.
    pub service_transition: Option<ServiceTableTransition>,
    /// Starts released by the failure's relationship reactions (BindsTo,
    /// OnFailure), exactly as after any other failure.
    pub start_dispatches: Vec<crate::execution::start::StartExecutionDispatch>,
}

impl SupervisorInternalErrorDispatch {
    pub fn service(&self) -> Option<&str> {
        self.subject.service.as_deref()
    }

    /// The one sentence the console prints and the audit event carries for
    /// this failure, so the two records read alike.
    pub fn message(&self) -> String {
        let subject = match (self.service(), self.subject.job_id) {
            (Some(service), _) => format!("service {service}"),
            (None, Some(job_id)) => format!("job {job_id}"),
            (None, None) => "no attributable service".to_string(),
        };
        let outcome = match (&self.service_transition, self.service()) {
            (Some(_), _) => "; the service is failed",
            (None, Some(_)) => "; the service keeps its state",
            (None, None) => "",
        };
        format!(
            "peinit: {subject}: internal error at {}: {}{outcome}",
            self.step, self.error
        )
    }
}

impl Supervisor {
    /// The service and job a reaped child's exit is about.
    pub fn internal_error_subject_for_pid(&self, pid: u32) -> SupervisorInternalErrorSubject {
        let job_id = self.jobs.active_job_by_pid(pid).or_else(|| {
            self.pending_process_setups
                .values()
                .find(|setup| setup.process.pid == pid)
                .map(|setup| setup.job_id)
        });
        self.internal_error_subject_for_job(job_id)
    }

    /// The service and job a setup-status readiness is about.
    pub fn internal_error_subject_for_setup(
        &self,
        setup_status_fd: i32,
    ) -> SupervisorInternalErrorSubject {
        let job_id = self
            .pending_process_setups
            .get(&setup_status_fd)
            .map(|setup| setup.job_id);
        self.internal_error_subject_for_job(job_id)
    }

    fn internal_error_subject_for_job(
        &self,
        job_id: Option<JobId>,
    ) -> SupervisorInternalErrorSubject {
        SupervisorInternalErrorSubject {
            service: job_id
                .and_then(|id| self.jobs.get(id))
                .and_then(|job| job.service.clone()),
            job_id,
        }
    }

    /// Contain an internal error to its subject.
    ///
    /// The job is retired (its process killed with its cgroup, its record
    /// ended with a cause naming the step and the error, any setup still
    /// pending for it dropped), the operation on the service fails with the
    /// `internal_error` result, and the service goes to Failed under
    /// `InternalError` — with the reactions any failure has: health and
    /// watchdog cancelled, BindsTo and OnFailure relationships applied.
    ///
    /// Nothing here can fail the caller. A reaction that itself raises is
    /// skipped and named in the returned error text; the core of the
    /// containment is committed regardless.
    pub fn fail_after_internal_error<P>(
        &mut self,
        subject: SupervisorInternalErrorSubject,
        step: &'static str,
        error: String,
        now_ns: u64,
        controller: &mut P,
    ) -> SupervisorInternalErrorDispatch
    where
        P: ProcessController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);
        let mut subject = subject;
        if subject.service.is_none() {
            subject.service = subject
                .job_id
                .and_then(|id| work.jobs.get(id))
                .and_then(|job| job.service.clone());
        }
        let cause = internal_error_result(format!("{step}: {error}"));
        let mut error = error;

        let job_event = subject
            .job_id
            .and_then(|job_id| retire_job(&mut work, job_id, now_ns, &cause, controller));
        let service_job_event = subject
            .service
            .as_deref()
            .and_then(|service| work.jobs.current_service_main_job(service))
            .filter(|main| Some(*main) != subject.job_id)
            .and_then(|main| {
                let event = retire_job(&mut work, main, now_ns, &cause, controller)?;
                // The service tree was killed with the job; its cgroup is
                // reclaimed on the same schedule as after a health
                // escalation, so a tree that will not drain is reported
                // rather than forgotten.
                record_cgroup_cleanup(
                    &mut work.cgroup_cleanup,
                    event.service.clone().unwrap_or_default(),
                    event.cgroup_id.clone(),
                    CgroupCleanupKind::ServiceTree,
                    now_ns,
                    self.settings.shutdown.post_kill_timeout_secs,
                );
                Some(event)
            });

        let operation_event = subject.service.as_deref().and_then(|service| {
            let pending = work
                .operations
                .current_for_service(service)
                .filter(|operation| {
                    matches!(
                        operation.state,
                        OperationState::Pending | OperationState::Running
                    )
                })
                .map(|operation| operation.id)?;
            work.operations
                .fail_operation(pending, now_ns, cause.clone())
                .ok()
        });

        let service_transition = subject
            .service
            .as_deref()
            .and_then(|service| fail_service(&mut work, service));
        if let Some(service) = subject.service.as_deref() {
            clear_service_deadlines(&mut work, service);
        }

        let mut start_dispatches = Vec::new();
        if let Some(transition) = &service_transition {
            let transitions = std::slice::from_ref(transition);
            apply_health_scheduling_after_transitions(&mut work, transitions, now_ns);
            apply_watchdog_scheduling_after_transitions(&mut work, transitions, now_ns);
            // Reactions are the one part that can raise. They run on a copy
            // so a refusal leaves the containment itself intact.
            let mut reacted = work.clone();
            match apply_relationship_reactions_after_transitions(
                &mut reacted,
                transitions,
                now_ns,
                self.settings.phase2.max_parallel_starts,
            ) {
                Ok(dispatches) => {
                    reacted.queue_start_dispatches(&dispatches);
                    start_dispatches = dispatches;
                    work = reacted;
                }
                Err(reaction_error) => {
                    error.push_str(&format!(
                        "; relationship reactions not applied: {reaction_error:?}"
                    ));
                }
            }
        }

        work.commit(self);
        SupervisorInternalErrorDispatch {
            step,
            subject,
            error,
            observed_at_ns: now_ns,
            job_event,
            service_job_event,
            operation_event,
            service_transition,
            start_dispatches,
        }
    }
}

/// End the job's record and release what it held, whichever state it is in.
fn retire_job<P>(
    work: &mut SupervisorWork,
    job_id: JobId,
    now_ns: u64,
    cause: &str,
    controller: &mut P,
) -> Option<JobEvent>
where
    P: ProcessController + ?Sized,
{
    let job = work.jobs.get(job_id).cloned()?;
    // Whatever the process is doing, it is no longer supervised: kill it
    // with its cgroup. Best effort — a kill that fails is the cgroup
    // cleanup's problem, not a reason to leave the record behind.
    let _ = controller.kill_cgroup(&job.cgroup_id);
    if let Some(setup) = work
        .pending_process_setups
        .iter()
        .find(|(_, setup)| setup.job_id == job_id)
        .map(|(fd, _)| *fd)
        .and_then(|fd| work.pending_process_setups.remove(&fd))
    {
        // The setup-status descriptor is the runtime's: it is registered
        // with epoll, and the runtime unregisters and closes it when it
        // learns the setup is gone. The rest of the process's descriptors
        // are released here, as on the pre-exec failure path.
        close_fd(setup.process.pidfd);
        if let Some(fd) = setup.process.stdout_fd {
            close_fd(fd);
        }
        if let Some(fd) = setup.process.stderr_fd {
            close_fd(fd);
        }
    }
    match job.state {
        JobState::Created => work.jobs.fail_job_before_start(job_id, now_ns, cause).ok(),
        JobState::Running => work.jobs.fail_running_job(job_id, now_ns, None, cause).ok(),
        _ => None,
    }
}

/// `-> Failed` under `InternalError`, for the states that can fail.
fn fail_service(work: &mut SupervisorWork, service: &str) -> Option<ServiceTableTransition> {
    let state = work.services.runtime(service)?.state;
    if !matches!(
        state,
        ServiceState::Starting
            | ServiceState::Active
            | ServiceState::Reloading
            | ServiceState::Stopping
            | ServiceState::Backoff
    ) {
        return None;
    }
    work.services
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Failed,
                cause: TransitionCause::InternalError,
            },
        )
        .ok()
}

/// Drop every lifecycle deadline that named the service.
///
/// A deadline is derived from the stores, and the path that raised did not
/// commit, so its deadline is still there. Left in place it would fire again
/// on the next timer turn, against a service that is now Failed, and raise
/// again — for ever.
fn clear_service_deadlines(work: &mut SupervisorWork, service: &str) {
    let operations = work
        .operations
        .active_records()
        .iter()
        .filter(|operation| operation.service == service)
        .map(|operation| operation.id)
        .collect::<Vec<_>>();
    for operation_id in operations {
        work.control.remove_stop_timeout(operation_id);
        work.start.remove_readiness_deadline(operation_id);
        work.start.remove_pre_start_hook_deadline(operation_id);
        work.start.remove_post_start_hook_deadline(operation_id);
        work.start.remove_pre_start_check_deadline(operation_id);
    }
    let _ = work.control.cancel_reload_for_service(service);
    let _ = work.start.cancel_post_start_for_service(service);
    work.health.cancel_service(service);
    work.watchdog.cancel_service(service);
}

fn close_fd(fd: i32) {
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
    }
}
