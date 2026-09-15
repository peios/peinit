use crate::boundary::{ProcessController, ShutdownFinalizer};
use crate::job::{JobExit, JobState, JobType, service_cgroup_root_path};
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{
    ErrorControl, RestartEvaluationAction, ServiceDefinition, evaluate_restart_after_failure,
};
use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::critical_budget::CriticalRebootTrigger;
use crate::supervisor::dispatch::{
    SupervisorWatchdogTimeoutDispatch, SupervisorWatchdogTimeoutOutcome,
};
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

use super::{WatchdogDeadline, WatchdogError};

const NANOS_PER_SEC: u64 = 1_000_000_000;

impl Supervisor {
    pub fn process_due_watchdog_timeouts<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Vec<SupervisorWatchdogTimeoutDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        self.process_due_watchdog_timeouts_with_finalizer(controller, None, now_ns)
    }

    pub fn process_due_watchdog_timeouts_with_finalizer<P>(
        &mut self,
        controller: &mut P,
        finalizer: Option<&mut dyn ShutdownFinalizer>,
        now_ns: u64,
    ) -> Result<Vec<SupervisorWatchdogTimeoutDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let due = self.watchdog.due_deadlines(now_ns);
        let mut work = SupervisorWork::from_supervisor(self);
        let mut dispatches = Vec::with_capacity(due.len());
        let mut critical_reboot_index = None;

        for deadline in due {
            let dispatch =
                process_watchdog_deadline_in_work(&mut work, controller, deadline, now_ns)
                    .map_err(SupervisorError::Watchdog)?;
            let critical_reboot_due = watchdog_critical_reboot_due(&work, &dispatch);
            if critical_reboot_index.is_none() && critical_reboot_due {
                critical_reboot_index = Some(dispatches.len());
            }
            if !critical_reboot_due {
                apply_relationship_reactions_after_transitions(
                    &mut work,
                    &dispatch.service_transitions,
                    now_ns,
                    self.settings.phase2.max_parallel_starts,
                )?;
            }
            if let Some(cgroup_id) = &dispatch.killed_cgroup_id {
                record_cgroup_cleanup(
                    &mut work.cgroup_cleanup,
                    &dispatch.service,
                    cgroup_id,
                    CgroupCleanupKind::ServiceTree,
                    now_ns,
                    self.settings.shutdown.post_kill_timeout_secs,
                );
            }
            dispatches.push(dispatch);
        }

        work.commit(self);
        if let Some(index) = critical_reboot_index {
            if let Some(finalizer) = finalizer {
                dispatches[index].critical_reboot = Some(self.critical_reboot(finalizer, now_ns)?);
            } else {
                let service = dispatches[index].service.clone();
                self.note_deferred_critical_reboot(
                    &service,
                    CriticalRebootTrigger::WatchdogTimeout,
                    Some(now_ns),
                );
            }
        }
        Ok(dispatches)
    }
}

fn process_watchdog_deadline_in_work(
    work: &mut SupervisorWork,
    controller: &mut (impl ProcessController + ?Sized),
    deadline: WatchdogDeadline,
    now_ns: u64,
) -> Result<SupervisorWatchdogTimeoutDispatch, WatchdogError> {
    if !active_current_generation(work, &deadline) {
        work.watchdog.cancel_service(&deadline.service);
        return Ok(stale_dispatch(deadline, now_ns));
    }

    let definition = work
        .services
        .definition(&deadline.service)
        .cloned()
        .ok_or_else(|| WatchdogError::UnknownService {
            service: deadline.service.clone(),
        })?;
    let job_id = work
        .jobs
        .current_service_main_job(&deadline.service)
        .ok_or_else(|| WatchdogError::MissingMainJob {
            service: deadline.service.clone(),
        })?;
    let job = work
        .jobs
        .get(job_id)
        .cloned()
        .ok_or(crate::job::JobStoreError::UnknownJob { id: job_id })
        .map_err(WatchdogError::JobStore)?;
    if job.job_type != JobType::ServiceMain || job.state != JobState::Running {
        return Err(WatchdogError::MissingMainJob {
            service: deadline.service.clone(),
        });
    }

    work.watchdog.cancel_service(&deadline.service);
    let killed_cgroup_id = service_cgroup_root_path(&deadline.service, deadline.cgroup_generation);
    controller
        .kill_cgroup(&killed_cgroup_id)
        .map_err(WatchdogError::Boundary)?;
    let job_event = work
        .jobs
        .fail_running_job(
            job_id,
            now_ns,
            Some(JobExit::Signal(9)),
            "watchdog timed out",
        )
        .map_err(WatchdogError::JobStore)?;
    let transition =
        apply_watchdog_timeout_transition(work, &deadline.service, &definition, now_ns)?;
    let outcome = if transition.event.to == ServiceState::Backoff {
        SupervisorWatchdogTimeoutOutcome::RestartScheduled
    } else {
        SupervisorWatchdogTimeoutOutcome::Failed
    };

    Ok(SupervisorWatchdogTimeoutDispatch {
        service: deadline.service,
        generation: deadline.generation,
        job_event: Some(job_event),
        outcome,
        service_transitions: vec![transition],
        killed_cgroup_id: Some(killed_cgroup_id),
        timed_out_at_ns: now_ns,
        critical_reboot: None,
    })
}

fn watchdog_critical_reboot_due(
    work: &SupervisorWork,
    dispatch: &SupervisorWatchdogTimeoutDispatch,
) -> bool {
    let Some(definition) = work.services.definition(&dispatch.service) else {
        return false;
    };
    definition.error_control == ErrorControl::Critical
        && dispatch.service_transitions.iter().any(|transition| {
            transition.event.to == ServiceState::Failed
                && transition.event.cause == TransitionCause::RestartBudgetExhausted
        })
}

fn apply_watchdog_timeout_transition(
    work: &mut SupervisorWork,
    service: &str,
    definition: &ServiceDefinition,
    now_ns: u64,
) -> Result<crate::service::ServiceTableTransition, WatchdogError> {
    let restart_failures = work
        .services
        .runtime(service)
        .ok_or_else(|| WatchdogError::UnknownService {
            service: service.to_string(),
        })?
        .consecutive_restart_failures;
    let evaluation = evaluate_restart_after_failure(
        definition,
        TransitionCause::WatchdogTimeout,
        None,
        restart_failures,
    );
    match evaluation.action {
        RestartEvaluationAction::Backoff {
            cause, delay_secs, ..
        } => work
            .services
            .transition_service_to_restart_backoff(
                service,
                cause,
                now_ns.saturating_add(delay_secs.saturating_mul(NANOS_PER_SEC)),
            )
            .map_err(WatchdogError::ServiceTable),
        RestartEvaluationAction::Fail { cause, state } => work
            .services
            .transition_service(service, ServiceTransition { to: state, cause })
            .map_err(WatchdogError::ServiceTable),
    }
}

fn active_current_generation(work: &SupervisorWork, deadline: &WatchdogDeadline) -> bool {
    work.services
        .runtime(&deadline.service)
        .is_some_and(|runtime| {
            runtime.state == ServiceState::Active && runtime.generation == deadline.generation
        })
}

fn stale_dispatch(deadline: WatchdogDeadline, now_ns: u64) -> SupervisorWatchdogTimeoutDispatch {
    SupervisorWatchdogTimeoutDispatch {
        service: deadline.service,
        generation: deadline.generation,
        job_event: None,
        outcome: SupervisorWatchdogTimeoutOutcome::Stale,
        service_transitions: Vec::new(),
        killed_cgroup_id: None,
        timed_out_at_ns: now_ns,
        critical_reboot: None,
    }
}
