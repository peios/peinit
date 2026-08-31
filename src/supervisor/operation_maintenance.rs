mod deadlines;
mod model;
mod on_failure_chain;
mod pending_timeout;
mod restart_window;
mod service_main_start_timeout;

pub use model::SupervisorOperationMaintenanceTurn;
pub use on_failure_chain::SupervisorOnFailureChainSettledDispatch;

use crate::boundary::ProcessController;

use super::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;
use pending_timeout::fail_pending_operation_timeout;
use on_failure_chain::settle_due_on_failure_chains;
use restart_window::reset_due_restart_windows;
use service_main_start_timeout::process_due_service_main_start_timeout;

impl Supervisor {
    pub fn process_due_operation_maintenance(
        &mut self,
        now_ns: u64,
    ) -> Result<SupervisorOperationMaintenanceTurn, SupervisorError> {
        self.process_due_operation_maintenance_inner(None, now_ns)
    }

    pub fn process_due_operation_maintenance_with_controller(
        &mut self,
        controller: &mut dyn ProcessController,
        now_ns: u64,
    ) -> Result<SupervisorOperationMaintenanceTurn, SupervisorError> {
        self.process_due_operation_maintenance_inner(Some(controller), now_ns)
    }

    fn process_due_operation_maintenance_inner(
        &mut self,
        controller: Option<&mut dyn ProcessController>,
        now_ns: u64,
    ) -> Result<SupervisorOperationMaintenanceTurn, SupervisorError> {
        let mut turn = SupervisorOperationMaintenanceTurn::default();
        let due = self.due_operation_maintenance(controller.is_some(), now_ns);
        if due.requires_supervisor_work() {
            let mut work = SupervisorWork::from_supervisor(self);
            for operation in due.pending_operation_timeouts {
                if work
                    .operations
                    .get(operation.id)
                    .is_none_or(|current| current.state.is_terminal())
                {
                    continue;
                }
                let timeout = fail_pending_operation_timeout(&mut work, operation.id, now_ns)?;
                turn.operation_timeouts.extend(timeout.operation_events);
                turn.graph_events.extend(timeout.graph_events);
            }
            if let Some(controller) = controller {
                for timeout in due.running_service_main_start_timeouts {
                    if work
                        .operations
                        .get(timeout.operation_id)
                        .is_none_or(|operation| operation.state.is_terminal())
                    {
                        continue;
                    }
                    let timeout = process_due_service_main_start_timeout(
                        &mut work,
                        controller,
                        timeout,
                        now_ns,
                        self.settings.phase2.max_parallel_starts,
                    )?;
                    turn.operation_timeouts
                        .extend(timeout.timeout.operation_events.clone());
                    turn.graph_events
                        .extend(timeout.timeout.graph_events.clone());
                    turn.start_dispatches.extend(timeout.start_dispatches);
                    if let Some(cgroup_id) = &timeout.timeout.killed_cgroup_id
                        && let Some(service) = timeout.timeout.job_event.service.as_deref()
                    {
                        record_cgroup_cleanup(
                            &mut work.cgroup_cleanup,
                            service,
                            cgroup_id,
                            CgroupCleanupKind::ServiceTree,
                            now_ns,
                            self.settings.shutdown.post_kill_timeout_secs,
                        );
                    }
                    turn.service_main_start_timeouts.push(timeout.timeout);
                }
            }
            turn.restart_window_resets =
                reset_due_restart_windows(&mut work, due.restart_window_resets, now_ns)?;
            turn.on_failure_chain_settles =
                settle_due_on_failure_chains(&mut work, due.on_failure_chain_settles, now_ns)?;
            work.commit(self);
        }

        turn.relationship_audit_events = self.relationships.drain_audit_events();
        // With the turn's work done, every graph context whose members have
        // all reached a terminal status is bookkeeping no reader can reach.
        // Reclaimed here alongside the other retention sweeps, and at a turn
        // boundary rather than inline, so nothing that was still walking the
        // events of this turn loses its context (PEI-364).
        turn.retired_graph_contexts = self.graph.retire_drained_contexts();
        turn.purged_operations = self.purge_retained_terminal_operations(now_ns);
        turn.purged_jobs = self.submitted.purge_retained_until(now_ns);
        Ok(turn)
    }
}
