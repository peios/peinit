use crate::boundary::{BootAttemptCounter, ProcessController, ShutdownFinalizer};

use super::critical_reboot::annotate_critical_reboot_if_due;
use super::{
    SupervisorLifecycleDeadline, SupervisorLifecycleDeadlineDispatch,
    SupervisorLifecycleDeadlineHoldoff, SupervisorLifecycleDeadlineKind,
};
use crate::control::lifecycle::LifecycleCommand;
use crate::supervisor::SupervisorInternalErrorSubject;
use crate::supervisor::dispatch::{
    SupervisorBootSettleDispatch, SupervisorBootSettleFailure, SupervisorBootSettleStart,
};
use crate::supervisor::state::{Supervisor, SupervisorError};

/// How long a deadline that raised and could not be removed is held back
/// before it is acted on — and its containment announced — again. The same
/// once-a-second pacing as progress events.
const RAISED_DEADLINE_RETRY_INTERVAL_NS: u64 = crate::submitted::PROGRESS_EVENT_INTERVAL_NS;

impl Supervisor {
    pub fn process_due_lifecycle_deadlines<P, B>(
        &mut self,
        controller: &mut P,
        boot_attempt_counter: &mut B,
        now_ns: u64,
    ) -> Result<Option<SupervisorLifecycleDeadlineDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
        B: BootAttemptCounter + ?Sized,
    {
        self.process_due_lifecycle_deadlines_with_finalizer(
            controller,
            boot_attempt_counter,
            None,
            now_ns,
        )
    }

    pub fn process_due_lifecycle_deadlines_with_finalizer<P, B>(
        &mut self,
        controller: &mut P,
        boot_attempt_counter: &mut B,
        finalizer: Option<&mut dyn ShutdownFinalizer>,
        now_ns: u64,
    ) -> Result<Option<SupervisorLifecycleDeadlineDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
        B: BootAttemptCounter + ?Sized,
    {
        if self.shutdown().is_some() {
            return Ok(None);
        }

        let mut dispatch = SupervisorLifecycleDeadlineDispatch::default();
        let mut contained = std::collections::BTreeSet::new();
        // A holdoff outlives its end by one interval, so the retry it paced
        // still finds it and can tell an identical repeat from news.
        self.lifecycle_deadline_holdoffs.retain(|holdoff| {
            holdoff
                .not_before_ns
                .saturating_add(RAISED_DEADLINE_RETRY_INTERVAL_NS)
                > now_ns
        });
        // The deadline as its store holds it — the holdoffs are keyed by that,
        // not by the time they push it to.
        while let Some(deadline) = self.next_scheduled_lifecycle_deadline() {
            if self.held_due_at_ns(&deadline) > now_ns {
                break;
            }
            if self.drop_deadline_of_finished_operation(&deadline.kind) {
                continue;
            }
            let subject = SupervisorInternalErrorSubject {
                service: Some(deadline.kind.service().to_string()).filter(|s| !s.is_empty()),
                job_id: deadline.kind.job_id(),
            };
            let key = (deadline.kind.rank(), subject.clone());
            let progressed = match self.process_due_lifecycle_deadline(
                deadline.kind.clone(),
                controller,
                boot_attempt_counter,
                now_ns,
                &mut dispatch,
            ) {
                Ok(progressed) => progressed,
                // A deadline about one service that peinit could not act on
                // fails that service, not the loop (PEI-1125). Containment
                // clears the service's deadlines, and the deadline that
                // raised is then removed outright: left in place it is due
                // again on the very next turn, the timer re-arms in the past,
                // and the same containment is announced for ever (PEI-1267).
                // One the stores cannot drop is held off instead, so it is
                // retried at most once an interval, and a retry that raises
                // the same error and changes nothing is not announced again.
                Err(error) if subject.is_attributable() && !contained.contains(&key) => {
                    contained.insert(key);
                    let failure = self.fail_after_internal_error(
                        subject,
                        "lifecycle deadline",
                        format!("{error:?}"),
                        now_ns,
                        controller,
                    );
                    let repeat = failure.changed_nothing()
                        && self.lifecycle_deadline_holdoffs.iter().any(|holdoff| {
                            holdoff.deadline == deadline && holdoff.error == failure.error
                        });
                    self.retire_raised_lifecycle_deadline(&deadline, &failure.error, now_ns);
                    if !repeat {
                        dispatch.internal_errors.push(failure);
                    }
                    true
                }
                // The same service raised again from this deadline kind in
                // this turn: it has been announced and failed once already.
                // Retire this one too, and leave the rest for the next turn
                // rather than risk going round again here.
                Err(error) if subject.is_attributable() => {
                    self.retire_raised_lifecycle_deadline(&deadline, &format!("{error:?}"), now_ns);
                    false
                }
                Err(error) => return Err(error),
            };
            if !progressed {
                break;
            }
        }

        annotate_critical_reboot_if_due(self, &mut dispatch, finalizer, now_ns)?;

        Ok((!dispatch.is_empty()).then_some(dispatch))
    }

    /// Drop a due deadline whose operation has already ended, instead of
    /// acting on it.
    ///
    /// A readiness deadline times out a start; once that start has been
    /// aborted, cancelled or otherwise finished there is nothing left for it
    /// to time out, and acting on it can only fail — the start-failure path
    /// refuses an operation that is not Running, and that refusal was being
    /// contained as an internal error against whatever the service was doing
    /// by then (PEI-1267). Only readiness is dropped here: its deadline holds
    /// nothing but itself. The other operation deadlines hold a hook job, a
    /// check helper's descriptors or a reload command, which a bare drop
    /// would orphan.
    fn drop_deadline_of_finished_operation(
        &mut self,
        kind: &SupervisorLifecycleDeadlineKind,
    ) -> bool {
        let SupervisorLifecycleDeadlineKind::ReadinessTimeout { operation_id, .. } = kind else {
            return false;
        };
        let finished = self
            .operations
            .get(*operation_id)
            .is_none_or(|operation| operation.state.is_terminal());
        finished
            && self
                .start
                .remove_readiness_deadline(*operation_id)
                .is_some()
    }

    /// Remove a deadline that raised from the store it is derived from, or
    /// hold it off for an interval if no store can drop it.
    ///
    /// The holdoff is short on purpose: the next deadline is the earliest of
    /// all of them, so while it is held every other deadline waits too.
    fn retire_raised_lifecycle_deadline(
        &mut self,
        deadline: &SupervisorLifecycleDeadline,
        error: &str,
        now_ns: u64,
    ) {
        self.remove_lifecycle_deadline(&deadline.kind);
        if self.next_scheduled_lifecycle_deadline().as_ref() != Some(deadline) {
            return;
        }
        self.lifecycle_deadline_holdoffs
            .retain(|holdoff| holdoff.deadline != *deadline);
        self.lifecycle_deadline_holdoffs
            .push(SupervisorLifecycleDeadlineHoldoff {
                deadline: deadline.clone(),
                not_before_ns: now_ns.saturating_add(RAISED_DEADLINE_RETRY_INTERVAL_NS),
                error: error.to_string(),
            });
    }

    fn remove_lifecycle_deadline(&mut self, kind: &SupervisorLifecycleDeadlineKind) {
        match kind {
            SupervisorLifecycleDeadlineKind::PreStartCheckTimeout { operation_id, .. } => {
                self.start.remove_pre_start_check_deadline(*operation_id);
            }
            SupervisorLifecycleDeadlineKind::PreStartHookTimeout { operation_id, .. } => {
                self.start.remove_pre_start_hook_deadline(*operation_id);
            }
            SupervisorLifecycleDeadlineKind::PostStartHookTimeout { operation_id, .. } => {
                self.start.remove_post_start_hook_deadline(*operation_id);
            }
            SupervisorLifecycleDeadlineKind::ReadinessTimeout { operation_id, .. } => {
                self.start.remove_readiness_deadline(*operation_id);
            }
            SupervisorLifecycleDeadlineKind::StopTimeout { operation_id, .. } => {
                self.control.remove_stop_timeout(*operation_id);
            }
            SupervisorLifecycleDeadlineKind::ReloadDetection { operation_id, .. } => {
                self.control.remove_reload_detection_deadline(*operation_id);
            }
            SupervisorLifecycleDeadlineKind::ReloadCommandTimeout { operation_id, .. } => {
                self.control.remove_reload_command_deadline(*operation_id);
            }
            SupervisorLifecycleDeadlineKind::CgroupCleanup { cgroup_id, .. } => {
                self.cgroup_cleanup.remove(cgroup_id);
            }
            SupervisorLifecycleDeadlineKind::HealthCheckInterval { service, .. }
            | SupervisorLifecycleDeadlineKind::HealthCheckTimeout { service, .. } => {
                self.health.cancel_service(service);
            }
            SupervisorLifecycleDeadlineKind::WatchdogTimeout { service, .. } => {
                self.watchdog.cancel_service(service);
            }
            // Derived from state the containment has already moved on (a
            // Backoff service is failed out of Backoff, a submitted job is
            // retired) or not attributable to a service at all. If one is
            // still due regardless, the holdoff paces it.
            SupervisorLifecycleDeadlineKind::RestartBackoff { .. }
            | SupervisorLifecycleDeadlineKind::SubmittedJob { .. }
            | SupervisorLifecycleDeadlineKind::BootSuccess
            | SupervisorLifecycleDeadlineKind::BootSettle => {}
        }
    }

    fn process_due_lifecycle_deadline<P, B>(
        &mut self,
        kind: SupervisorLifecycleDeadlineKind,
        controller: &mut P,
        boot_attempt_counter: &mut B,
        now_ns: u64,
        dispatch: &mut SupervisorLifecycleDeadlineDispatch,
    ) -> Result<bool, SupervisorError>
    where
        P: ProcessController + ?Sized,
        B: BootAttemptCounter + ?Sized,
    {
        match kind {
            SupervisorLifecycleDeadlineKind::PreStartCheckTimeout { operation_id, .. } => {
                let Some(timeout) =
                    self.process_due_filesystem_check_timeout(operation_id, now_ns, controller)?
                else {
                    return Ok(false);
                };
                dispatch.pre_start_check_timeouts.push(timeout);
            }
            SupervisorLifecycleDeadlineKind::PreStartHookTimeout { .. } => {
                let Some(timeout) =
                    self.process_next_due_pre_start_hook_timeout(controller, now_ns)?
                else {
                    return Ok(false);
                };
                dispatch.pre_start_hook_timeouts.push(timeout);
            }
            SupervisorLifecycleDeadlineKind::PostStartHookTimeout { .. } => {
                let Some(timeout) =
                    self.process_next_due_post_start_hook_timeout(controller, now_ns)?
                else {
                    return Ok(false);
                };
                dispatch.post_start_hook_timeouts.push(timeout);
            }
            SupervisorLifecycleDeadlineKind::ReadinessTimeout { .. } => {
                let Some(timeout) = self.process_next_due_readiness_timeout(controller, now_ns)?
                else {
                    return Ok(false);
                };
                dispatch.readiness_timeouts.push(timeout);
            }
            SupervisorLifecycleDeadlineKind::StopTimeout { .. } => {
                let Some(escalation) = self.process_next_due_stop_timeout(controller, now_ns)?
                else {
                    return Ok(false);
                };
                dispatch.stop_timeouts.push(escalation);
            }
            SupervisorLifecycleDeadlineKind::ReloadDetection { .. } => {
                let detections = self.process_due_reload_detection_windows(now_ns)?;
                if detections.is_empty() {
                    return Ok(false);
                }
                dispatch.reload_detections.extend(detections);
            }
            SupervisorLifecycleDeadlineKind::ReloadCommandTimeout { .. } => {
                let Some(timeout) =
                    self.process_next_due_reload_command_timeout(controller, now_ns)?
                else {
                    return Ok(false);
                };
                dispatch.reload_command_timeouts.push(timeout);
            }
            SupervisorLifecycleDeadlineKind::RestartBackoff { .. } => {
                let restarts = self.process_due_restart_backoffs(now_ns)?;
                let failures = self.take_restart_backoff_failures();
                if restarts.is_empty() && failures.is_empty() {
                    return Ok(false);
                }
                dispatch.restart_backoffs.extend(restarts);
                dispatch.restart_backoff_failures.extend(failures);
            }
            SupervisorLifecycleDeadlineKind::HealthCheckInterval { .. } => {
                let intervals = self.process_due_health_check_intervals(now_ns)?;
                if intervals.is_empty() {
                    return Ok(false);
                }
                dispatch.health_check_intervals.extend(intervals);
            }
            SupervisorLifecycleDeadlineKind::HealthCheckTimeout { .. } => {
                let timeouts = self.process_due_health_check_timeouts(controller, now_ns)?;
                if timeouts.is_empty() {
                    return Ok(false);
                }
                dispatch.health_check_timeouts.extend(timeouts);
            }
            SupervisorLifecycleDeadlineKind::WatchdogTimeout { .. } => {
                let timeouts = self.process_due_watchdog_timeouts(controller, now_ns)?;
                if timeouts.is_empty() {
                    return Ok(false);
                }
                dispatch.watchdog_timeouts.extend(timeouts);
            }
            SupervisorLifecycleDeadlineKind::CgroupCleanup { .. } => {
                let Some(leaks) = self.process_due_cgroup_cleanups(controller, now_ns)? else {
                    return Ok(false);
                };
                dispatch.cgroup_leaks.extend(leaks);
            }
            SupervisorLifecycleDeadlineKind::BootSuccess => {
                let Some(boot_success) =
                    self.boot_success
                        .process_due(&self.services, boot_attempt_counter, now_ns)
                else {
                    return Ok(false);
                };
                dispatch.boot_successes.push(boot_success);
            }
            SupervisorLifecycleDeadlineKind::SubmittedJob { job_id, kind } => {
                let Some(dispatched) =
                    self.process_due_submitted_job_deadline(job_id, kind, controller, now_ns)?
                else {
                    return Ok(false);
                };
                dispatch.submitted_jobs.push(dispatched);
            }
            SupervisorLifecycleDeadlineKind::BootSettle => {
                let Some(due) = self.boot_settle.take_due(&self.services, now_ns) else {
                    return Ok(false);
                };
                // Started one at a time and independently: these services have
                // no relationship to each other beyond both having waited, so
                // one refusing to start (disabled since boot, dependency since
                // failed) must not stop the others. A failure to start is
                // reported and dropped rather than propagated — this path is a
                // convenience for output legibility, and taking the boot down
                // over it would be a far worse outcome than a scrambled prompt.
                let mut started = Vec::new();
                let mut failed = Vec::new();
                for service in due.services {
                    match self.run_lifecycle_command_at(
                        LifecycleCommand::Start,
                        service.clone(),
                        None,
                        now_ns,
                    ) {
                        Ok(lifecycle) => started.push(SupervisorBootSettleStart {
                            service,
                            lifecycle: Box::new(lifecycle),
                        }),
                        Err(error) => failed.push(SupervisorBootSettleFailure {
                            service,
                            error: format!("{error:?}"),
                        }),
                    }
                }
                dispatch.boot_settles.push(SupervisorBootSettleDispatch {
                    started,
                    failed,
                    timed_out: due.timed_out,
                });
            }
        }
        Ok(true)
    }
}
