use crate::boundary::{Clock, ProcessController};
use crate::execution::control::{
    ControlExecutionContext, ControlExecutionDetail, ControlOperationKind, ControlOperationRequest,
    begin_control_operation,
};

use super::super::dispatch::SupervisorControlDispatch;
use super::super::health::apply_health_scheduling_after_transitions;
use super::super::relationships::apply_relationship_reactions_after_transitions;
use super::super::state::{Supervisor, SupervisorError};
use super::super::watchdog::apply_watchdog_scheduling_after_transitions;
use super::super::work::SupervisorWork;

impl Supervisor {
    pub fn execute_next_pending_control_operation<C, P>(
        &mut self,
        controller: &mut P,
        clock: &mut C,
    ) -> Result<Option<SupervisorControlDispatch>, SupervisorError>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
    {
        let observed_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
        let mut work = SupervisorWork::from_supervisor(self);

        while let Some(pending) = work.pending_control_operations.pop_front() {
            if work
                .operations
                .get(pending.operation_id)
                .is_none_or(|operation| operation.state.is_terminal())
            {
                continue;
            }

            let execution = begin_control_operation(
                &mut ControlExecutionContext {
                    services: &mut work.services,
                    operations: &mut work.operations,
                    jobs: &mut work.jobs,
                    job_ids: &mut work.job_ids,
                    control_store: &mut work.control,
                    controller,
                },
                ControlOperationRequest {
                    operation_id: pending.operation_id,
                    observed_at_ns,
                },
            )
            .map_err(SupervisorError::Control)?;
            if let ControlExecutionDetail::ReloadCommand { job_id, .. } = &execution.detail {
                work.pending_control_launches.push_back(*job_id);
            }
            // A stop that lands while the service's ExecStartPost hooks are
            // still running supersedes them: the sequence will never complete,
            // and its deadline must not fire later against a hooks cgroup the
            // stop has already torn down (PEI-491). Kill what is still running
            // now; the stop's own cleanup removes the tree.
            if matches!(
                execution.kind,
                ControlOperationKind::Stop | ControlOperationKind::RestartStopLeg
            ) {
                for deadline in work.start.cancel_post_start_for_service(&execution.service) {
                    controller
                        .kill_cgroup(&deadline.hooks_cgroup_id)
                        .map_err(|error| {
                            SupervisorError::Control(
                                crate::execution::control::ControlExecutionError::Boundary(error),
                            )
                        })?;
                }
            }
            apply_health_scheduling_after_transitions(
                &mut work,
                std::slice::from_ref(&execution.service_transition),
                observed_at_ns,
            );
            apply_watchdog_scheduling_after_transitions(
                &mut work,
                std::slice::from_ref(&execution.service_transition),
                observed_at_ns,
            );
            apply_relationship_reactions_after_transitions(
                &mut work,
                std::slice::from_ref(&execution.service_transition),
                observed_at_ns,
                self.settings().phase2.max_parallel_starts,
            )?;
            work.commit(self);
            return Ok(Some(SupervisorControlDispatch { execution }));
        }

        work.commit(self);
        Ok(None)
    }
}
