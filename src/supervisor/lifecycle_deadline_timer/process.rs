use crate::boundary::{BootAttemptCounter, ProcessController, ShutdownFinalizer};

use super::critical_reboot::annotate_critical_reboot_if_due;
use super::{SupervisorLifecycleDeadlineDispatch, SupervisorLifecycleDeadlineKind};
use crate::control::lifecycle::LifecycleCommand;
use crate::supervisor::dispatch::{
    SupervisorBootSettleDispatch, SupervisorBootSettleFailure, SupervisorBootSettleStart,
};
use crate::supervisor::state::{Supervisor, SupervisorError};

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
        while let Some(deadline) = self.next_lifecycle_deadline() {
            if deadline.due_at_ns > now_ns {
                break;
            }
            let progressed = self.process_due_lifecycle_deadline(
                deadline.kind,
                controller,
                boot_attempt_counter,
                now_ns,
                &mut dispatch,
            )?;
            if !progressed {
                break;
            }
        }

        if let Some(finalizer) = finalizer {
            annotate_critical_reboot_if_due(self, &mut dispatch, finalizer, now_ns)?;
        }

        Ok((!dispatch.is_empty()).then_some(dispatch))
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
                if restarts.is_empty() {
                    return Ok(false);
                }
                dispatch.restart_backoffs.extend(restarts);
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
                if !self.process_due_cgroup_cleanups(controller, now_ns)? {
                    return Ok(false);
                }
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
