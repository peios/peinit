use crate::supervisor::dispatch::SupervisorHealthCheckIntervalDispatch;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

mod due;
mod scheduling;

use due::process_due_health_check_interval;
pub(in crate::supervisor) use scheduling::{
    apply_health_scheduling_after_post_start, apply_health_scheduling_after_transitions,
};

impl Supervisor {
    pub fn process_due_health_check_intervals(
        &mut self,
        now_ns: u64,
    ) -> Result<Vec<SupervisorHealthCheckIntervalDispatch>, SupervisorError> {
        let due = self.health.due_interval_deadlines(now_ns);
        let mut work = SupervisorWork::from_supervisor(self);
        let mut dispatches = Vec::with_capacity(due.len());

        for deadline in due {
            let dispatch = process_due_health_check_interval(&mut work, deadline, now_ns)?;
            dispatches.push(dispatch);
        }

        work.commit(self);
        Ok(dispatches)
    }
}
