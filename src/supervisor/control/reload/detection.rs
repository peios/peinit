use crate::execution::control::complete_reload_detection_window;
use crate::supervisor::dispatch::SupervisorReloadDetectionDispatch;
use crate::supervisor::health::apply_health_scheduling_after_transitions;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::watchdog::apply_watchdog_scheduling_after_transitions;
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub fn process_due_reload_detection_windows(
        &mut self,
        now_ns: u64,
    ) -> Result<Vec<SupervisorReloadDetectionDispatch>, SupervisorError> {
        let due = self.control.due_reload_detection_deadlines(now_ns);
        let mut work = SupervisorWork::from_supervisor(self);
        let mut dispatches = Vec::with_capacity(due.len());

        for deadline in due {
            let completion = complete_reload_detection_window(
                &mut work.services,
                &mut work.operations,
                &mut work.control,
                deadline,
                now_ns,
            )
            .map_err(SupervisorError::Control)?;
            apply_health_scheduling_after_transitions(
                &mut work,
                std::slice::from_ref(&completion.service_transition),
                now_ns,
            );
            apply_watchdog_scheduling_after_transitions(
                &mut work,
                std::slice::from_ref(&completion.service_transition),
                now_ns,
            );
            apply_relationship_reactions_after_transitions(
                &mut work,
                std::slice::from_ref(&completion.service_transition),
                now_ns,
                self.settings().phase2.max_parallel_starts,
            )?;
            dispatches.push(SupervisorReloadDetectionDispatch { completion });
        }

        work.commit(self);
        Ok(dispatches)
    }
}
