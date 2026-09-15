use crate::boundary::ProcessController;
use crate::shutdown::{
    ShutdownError, ShutdownFinalizationState, ShutdownKind, ShutdownRuntime, plan_graceful_shutdown,
};

use super::dispatch::SupervisorShutdownDispatch;
use super::shutdown_completed::clear_completed_services;
use super::shutdown_starting::kill_starting_services;
use super::shutdown_wave::begin_first_stop_wave;
use super::state::{Supervisor, SupervisorError};
use super::submitted::{live_submitted_jobs_remain, stop_submitted_jobs_for_shutdown};
use super::work::SupervisorWork;

const NANOS_PER_SEC: u64 = 1_000_000_000;

impl Supervisor {
    pub fn begin_shutdown<P>(
        &mut self,
        kind: ShutdownKind,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<SupervisorShutdownDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        if let Some(shutdown) = &self.shutdown {
            return Err(SupervisorError::Shutdown(
                ShutdownError::AlreadyInProgress {
                    kind: shutdown.kind,
                },
            ));
        }

        let plan = plan_graceful_shutdown(&self.services)
            .map_err(ShutdownError::Plan)
            .map_err(SupervisorError::Shutdown)?;
        let mut work = SupervisorWork::from_supervisor(self);
        let mut stop_deadlines = Vec::new();

        let completed_transitions = clear_completed_services(&mut work, &plan)
            .map_err(ShutdownError::ServiceTable)
            .map_err(SupervisorError::Shutdown)?;
        let starting_kills = kill_starting_services(
            &mut work,
            &plan,
            controller,
            now_ns,
            self.settings.shutdown.post_kill_timeout_secs,
        )
        .map_err(SupervisorError::Shutdown)?;
        let first_wave =
            begin_first_stop_wave(&mut work, &plan, controller, now_ns, &mut stop_deadlines)
                .map_err(SupervisorError::Shutdown)?;
        let submitted_stops = stop_submitted_jobs_for_shutdown(
            &mut work,
            controller,
            now_ns,
            self.settings.shutdown.post_kill_timeout_secs,
        )?;

        let finalization = if plan.stop_waves.is_empty()
            && starting_kills.post_kill_deadlines.is_empty()
            && !live_submitted_jobs_remain(&work)
        {
            ShutdownFinalizationState::Ready
        } else {
            ShutdownFinalizationState::WaitingForServices
        };
        let runtime = ShutdownRuntime {
            kind,
            initiated_at_ns: now_ns,
            global_deadline_ns: shutdown_deadline_ns(
                now_ns,
                self.settings.shutdown.global_timeout_secs,
            ),
            plan,
            current_wave: 0,
            stop_deadlines,
            post_kill_deadlines: starting_kills.post_kill_deadlines,
            finalization,
        };
        work.shutdown = Some(runtime.clone());
        work.commit(self);

        Ok(SupervisorShutdownDispatch {
            runtime,
            completed_transitions,
            killed_starting: starting_kills.killed_starting,
            first_wave,
            submitted_stops,
            startup_operation_events: starting_kills.operation_events,
            startup_job_events: starting_kills.job_events,
            cancelled_setups: starting_kills.cancelled_setups,
        })
    }
}

fn shutdown_deadline_ns(started_at_ns: u64, timeout_secs: u64) -> u64 {
    started_at_ns.saturating_add(timeout_secs.saturating_mul(NANOS_PER_SEC))
}
