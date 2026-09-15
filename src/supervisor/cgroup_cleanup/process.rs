use crate::boundary::ProcessController;
use crate::service::runtime::LeakedCgroupKind;

use super::stop_main::{apply_stop_main_abandoned, apply_stop_main_empty};
use super::{
    CgroupCleanupDeadline, CgroupCleanupKind, SupervisorLeakedCgroupDispatch,
    cleanup_service_cgroup_tree,
};
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub(in crate::supervisor) fn next_cgroup_cleanup_deadline(
        &self,
    ) -> Option<CgroupCleanupDeadline> {
        self.cgroup_cleanup.next_deadline()
    }

    /// Returns the leaks newly recorded by this pass, or `None` if no deadline
    /// was actually due -- the caller distinguishes "nothing happened" from
    /// "a cleanup ran and reclaimed everything".
    pub(in crate::supervisor) fn process_due_cgroup_cleanups<P>(
        &mut self,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Option<Vec<SupervisorLeakedCgroupDispatch>>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let due = self.cgroup_cleanup.due_deadlines(now_ns);
        if due.is_empty() {
            return Ok(None);
        }

        let mut work = SupervisorWork::from_supervisor(self);
        let mut processed = false;
        let mut leaks = Vec::new();
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
                    &mut leaks,
                )?;
            } else {
                let start_dispatches = process_empty_cgroup_cleanup(
                    &mut work,
                    controller,
                    deadline,
                    now_ns,
                    self.settings.phase2.max_parallel_starts,
                    &mut leaks,
                )?;
                all_start_dispatches.extend(start_dispatches);
            }
        }
        work.queue_restart_start_dispatches(&all_start_dispatches);
        work.commit(self);
        Ok(processed.then_some(leaks))
    }
}

fn process_populated_cgroup_cleanup(
    work: &mut SupervisorWork,
    deadline: CgroupCleanupDeadline,
    now_ns: u64,
    max_parallel_starts: u32,
    leaks: &mut Vec<SupervisorLeakedCgroupDispatch>,
) -> Result<(), SupervisorError> {
    match deadline.kind {
        CgroupCleanupKind::Hooks => record_leaked_cgroup(
            work,
            &deadline.service,
            deadline.cgroup_id,
            LeakedCgroupKind::Hooks,
            now_ns,
            leaks,
        ),
        CgroupCleanupKind::Health => record_leaked_cgroup(
            work,
            &deadline.service,
            deadline.cgroup_id,
            LeakedCgroupKind::Health,
            now_ns,
            leaks,
        ),
        CgroupCleanupKind::Helper => record_leaked_cgroup(
            work,
            &deadline.service,
            deadline.cgroup_id,
            LeakedCgroupKind::Helper,
            now_ns,
            leaks,
        ),
        CgroupCleanupKind::ServiceTree => record_leaked_cgroup(
            work,
            &deadline.service,
            deadline.cgroup_id,
            LeakedCgroupKind::ServiceTree,
            now_ns,
            leaks,
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
            leaks,
        ),
    }
}

/// The cgroup read as unpopulated, so peinit is reclaiming the tree.
///
/// "Unpopulated" and "removable" are not the same question, which is the point
/// of `record_busy_cgroups` below.
fn process_empty_cgroup_cleanup<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    deadline: CgroupCleanupDeadline,
    now_ns: u64,
    max_parallel_starts: u32,
    leaks: &mut Vec<SupervisorLeakedCgroupDispatch>,
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
            let report = cleanup_service_cgroup_tree(controller, &deadline.cgroup_id)
                .map_err(SupervisorError::ProcessControl)?;
            record_busy_cgroups(work, &deadline.service, &report.busy, now_ns, leaks)?;
            Ok(Vec::new())
        }
        CgroupCleanupKind::StopMain {
            operation_id,
            root_cgroup_id,
        } => {
            let report = cleanup_service_cgroup_tree(controller, &root_cgroup_id)
                .map_err(SupervisorError::ProcessControl)?;
            record_busy_cgroups(work, &deadline.service, &report.busy, now_ns, leaks)?;
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

/// A cgroup that `rmdir` refused with `EBUSY` is a leak, and peinit used to
/// throw the answer away.
///
/// §4.1 names `rmdir` failing `EBUSY` as *the* trigger for a new generation.
/// The implementation triggered on `cgroup.events` reporting `populated == 1`
/// instead, and the two are not the same question: `populated` counts live
/// processes, `EBUSY` also covers a tree that cannot be removed for any other
/// reason. `CgroupCleanupReport.busy` was computed and carried all the way to
/// these callers, which then discarded it — so where the two diverged, the
/// next start reused a tree that already existed and was not empty, and the
/// service's processes joined whatever was still in it (PEI-353).
fn record_busy_cgroups(
    work: &mut SupervisorWork,
    service: &str,
    busy: &[String],
    now_ns: u64,
    leaks: &mut Vec<SupervisorLeakedCgroupDispatch>,
) -> Result<(), SupervisorError> {
    for cgroup_id in busy {
        record_leaked_cgroup(
            work,
            service,
            cgroup_id.clone(),
            busy_cgroup_kind(cgroup_id),
            now_ns,
            leaks,
        )?;
    }
    Ok(())
}

/// Which part of the service tree a busy path is, by its last component. The
/// root and `main/` are both the service tree proper: neither can be given up
/// without giving up the generation.
fn busy_cgroup_kind(cgroup_id: &str) -> LeakedCgroupKind {
    match std::path::Path::new(cgroup_id)
        .file_name()
        .and_then(|name| name.to_str())
    {
        Some("hooks") => LeakedCgroupKind::Hooks,
        Some("health") => LeakedCgroupKind::Health,
        _ => LeakedCgroupKind::ServiceTree,
    }
}

pub(super) fn record_leaked_cgroup(
    work: &mut SupervisorWork,
    service: &str,
    cgroup_id: String,
    kind: LeakedCgroupKind,
    now_ns: u64,
    leaks: &mut Vec<SupervisorLeakedCgroupDispatch>,
) -> Result<(), SupervisorError> {
    let recorded = work
        .services
        .record_leaked_cgroup(service, cgroup_id.clone(), kind, now_ns)
        .map_err(|error| {
            SupervisorError::Control(
                crate::execution::control::ControlExecutionError::ServiceTable(error),
            )
        })?;
    // Recording is idempotent, so only report a leak the first time it is seen.
    // Re-announcing the same path on every subsequent cleanup pass would turn
    // one broken disk into a stream of identical events.
    if recorded {
        leaks.push(SupervisorLeakedCgroupDispatch {
            service: service.to_string(),
            path: cgroup_id,
            kind,
            detected_at_ns: now_ns,
        });
    }
    Ok(())
}
