use crate::boundary::ProcessController;

use super::dispatch::SupervisorShutdownTimeoutDispatch;
use super::shutdown_progress::{advance_shutdown_progress, ensure_shutdown};
use super::shutdown_timeout_actions::{
    kill_all_remaining_services, process_due_post_kill_deadlines, process_due_stop_deadlines,
    shutdown_global_timeout_due,
};
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;

impl Supervisor {
    pub fn process_due_shutdown_timeouts<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Option<SupervisorShutdownTimeoutDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);
        ensure_shutdown(&work)?;
        let mut cgroup_kills = Vec::new();
        let mut job_events = Vec::new();
        let mut abandoned = Vec::new();
        let mut global_timeout = false;

        if shutdown_global_timeout_due(&work, now_ns) {
            global_timeout = true;
            cgroup_kills.extend(
                kill_all_remaining_services(
                    &mut work,
                    controller,
                    now_ns,
                    self.settings.shutdown.post_kill_timeout_secs,
                )
                .map_err(SupervisorError::Shutdown)?,
            );
        } else {
            cgroup_kills.extend(
                process_due_stop_deadlines(
                    &mut work,
                    controller,
                    now_ns,
                    self.settings.shutdown.post_kill_timeout_secs,
                )
                .map_err(SupervisorError::Shutdown)?,
            );
        }
        let post_kill = process_due_post_kill_deadlines(&mut work, controller, now_ns)
            .map_err(SupervisorError::Shutdown)?;
        let post_kill_processed = post_kill.processed > 0;
        job_events.extend(post_kill.job_events);
        abandoned.extend(post_kill.abandoned);

        if cgroup_kills.is_empty()
            && job_events.is_empty()
            && abandoned.is_empty()
            && !global_timeout
            && !post_kill_processed
        {
            return Ok(None);
        }

        let next_wave = advance_shutdown_progress(&mut work, controller, now_ns)
            .map_err(SupervisorError::Shutdown)?;
        let finalization = work
            .shutdown()
            .map_err(SupervisorError::Shutdown)?
            .finalization
            .clone();
        work.commit(self);

        Ok(Some(SupervisorShutdownTimeoutDispatch {
            global_timeout,
            cgroup_kills,
            job_events,
            abandoned,
            next_wave,
            finalization,
        }))
    }
}
