use crate::boundary::ProcessController;
use crate::service::runtime::LeakedCgroupKind;

use super::stop_main::{apply_stop_main_abandoned, apply_stop_main_empty};
use super::{CgroupCleanupDeadline, CgroupCleanupKind, cleanup_service_cgroup_tree};
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub(in crate::supervisor) fn next_cgroup_cleanup_deadline(
        &self,
    ) -> Option<CgroupCleanupDeadline> {
        self.cgroup_cleanup.next_deadline()
    }

    pub(in crate::supervisor) fn process_due_cgroup_cleanups<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<bool, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let due = self.cgroup_cleanup.due_deadlines(now_ns);
        if due.is_empty() {
            return Ok(false);
        }

        let mut work = SupervisorWork::from_supervisor(self);
        let mut processed = false;
        let mut all_start_dispatches = Vec::new();
        for deadline in due {
            let Some(deadline) = work.cgroup_cleanup.remove(&deadline.cgroup_id) else {
                continue;
            };
            processed = true;
            let populated = controller
                .cgroup_populated(&deadline.cgroup_id)
                .map_err(SupervisorError::ProcessControl)?;
            if populated {
                process_populated_cgroup_cleanup(
                    &mut work,
                    deadline,
                    now_ns,
                    self.settings.phase2.max_parallel_starts,
                )?;
            } else {
                let start_dispatches = process_empty_cgroup_cleanup(
                    &mut work,
                    controller,
                    deadline,
                    now_ns,
                    self.settings.phase2.max_parallel_starts,
                )?;
                all_start_dispatches.extend(start_dispatches);
            }
        }
        work.queue_restart_start_dispatches(&all_start_dispatches);
        work.commit(self);
        Ok(processed)
    }
}

fn process_populated_cgroup_cleanup(
    work: &mut SupervisorWork,
    deadline: CgroupCleanupDeadline,
    now_ns: u64,
    max_parallel_starts: u32,
) -> Result<(), SupervisorError> {
    match deadline.kind {
        CgroupCleanupKind::Hooks => record_leaked_cgroup(
            work,
            &deadline.service,
            deadline.cgroup_id,
            LeakedCgroupKind::Hooks,
            now_ns,
        ),
        CgroupCleanupKind::Health => record_leaked_cgroup(
            work,
            &deadline.service,
            deadline.cgroup_id,
            LeakedCgroupKind::Health,
            now_ns,
        ),
        CgroupCleanupKind::Helper => Ok(()),
        CgroupCleanupKind::ServiceTree => record_leaked_cgroup(
            work,
            &deadline.service,
            deadline.cgroup_id,
            LeakedCgroupKind::ServiceTree,
            now_ns,
        ),
        CgroupCleanupKind::StopMain {
            operation_id,
            root_cgroup_id,
        } => apply_stop_main_abandoned(
            work,
            &deadline.service,
            root_cgroup_id,
            operation_id,
            now_ns,
            max_parallel_starts,
        ),
    }
}

fn process_empty_cgroup_cleanup<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    deadline: CgroupCleanupDeadline,
    now_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<crate::execution::start::RestartStartExecutionDispatch>, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    match deadline.kind {
        CgroupCleanupKind::Hooks | CgroupCleanupKind::Health | CgroupCleanupKind::Helper => {
            controller
                .remove_cgroup(&deadline.cgroup_id)
                .map_err(SupervisorError::ProcessControl)?;
            Ok(Vec::new())
        }
        CgroupCleanupKind::ServiceTree => {
            cleanup_service_cgroup_tree(controller, &deadline.cgroup_id)
                .map_err(SupervisorError::ProcessControl)?;
            Ok(Vec::new())
        }
        CgroupCleanupKind::StopMain {
            operation_id,
            root_cgroup_id,
        } => {
            cleanup_service_cgroup_tree(controller, &root_cgroup_id)
                .map_err(SupervisorError::ProcessControl)?;
            apply_stop_main_empty(
                work,
                &deadline.service,
                operation_id,
                now_ns,
                max_parallel_starts,
            )
        }
    }
}

fn record_leaked_cgroup(
    work: &mut SupervisorWork,
    service: &str,
    cgroup_id: String,
    kind: LeakedCgroupKind,
    now_ns: u64,
) -> Result<(), SupervisorError> {
    work.services
        .record_leaked_cgroup(service, cgroup_id, kind, now_ns)
        .map_err(|error| {
            SupervisorError::Control(
                crate::execution::control::ControlExecutionError::ServiceTable(error),
            )
        })
}
