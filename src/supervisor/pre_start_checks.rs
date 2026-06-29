use crate::boundary::{
    FilesystemCheckHelperLauncher, FilesystemCheckHelperRequest, FilesystemCheckReport,
    LaunchedFilesystemCheckHelper,
};
use crate::execution::start::{
    PreStartCheckCompletionContext, StartExecutionContext, complete_pre_start_check_helper,
    timeout_pre_start_check_helper,
};
use crate::ids::OperationId;

use super::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use super::dispatch::{
    SupervisorFilesystemCheckCompletionDispatch, SupervisorFilesystemCheckLaunchDispatch,
    SupervisorFilesystemCheckTimeoutDispatch,
};
use super::relationships::apply_relationship_reactions_after_transitions;
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;

impl Supervisor {
    pub fn filesystem_check_helper_by_result_fd(
        &self,
        result_fd: i32,
    ) -> Option<LaunchedFilesystemCheckHelper> {
        self.start
            .running_pre_start_check_helper_by_result_fd(result_fd)
            .map(|running| running.helper.clone())
    }

    pub fn filesystem_check_helper_by_pidfd(
        &self,
        pidfd: i32,
    ) -> Option<LaunchedFilesystemCheckHelper> {
        self.start
            .running_pre_start_check_helper_by_pidfd(pidfd)
            .map(|running| running.helper.clone())
    }

    pub fn launch_next_pending_filesystem_check_helper<L>(
        &mut self,
        launcher: &mut L,
        launched_at_ns: u64,
    ) -> Result<Option<SupervisorFilesystemCheckLaunchDispatch>, SupervisorError>
    where
        L: FilesystemCheckHelperLauncher + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);
        let Some(pending) = work.start.pop_pending_pre_start_check_launch() else {
            return Ok(None);
        };

        let helper = launcher
            .launch_filesystem_check_helper(FilesystemCheckHelperRequest {
                service: pending.service.clone(),
                operation_id: pending.operation_id,
                cgroup_id: pending.helper_cgroup_id(),
                checks: pending.checks,
            })
            .map_err(SupervisorError::FilesystemCheck)?;
        work.start
            .record_running_pre_start_check_helper(helper.clone(), launched_at_ns);

        work.commit(self);
        Ok(Some(SupervisorFilesystemCheckLaunchDispatch { helper }))
    }

    pub fn complete_filesystem_check_helper(
        &mut self,
        result_fd: i32,
        report: FilesystemCheckReport,
        observed_at_ns: u64,
    ) -> Result<SupervisorFilesystemCheckCompletionDispatch, SupervisorError> {
        let mut work = SupervisorWork::from_supervisor(self);
        let completion = complete_pre_start_check_helper(
            &mut PreStartCheckCompletionContext {
                services: &mut work.services,
                operations: &mut work.operations,
                graph: &mut work.graph,
                jobs: &mut work.jobs,
                job_ids: &mut work.job_ids,
                start_store: &mut work.start,
            },
            result_fd,
            report,
        )
        .map_err(SupervisorError::Start)?;

        if let Some(job_event) = &completion.job_event {
            work.queue_created_start_job(job_event);
        }
        let mut start_dispatches = apply_relationship_reactions_after_transitions(
            &mut work,
            &completion.service_transitions,
            observed_at_ns,
            self.settings.phase2.max_parallel_starts,
        )?;
        start_dispatches.extend(work.release_after_graph_events(
            &completion.graph_events,
            self.settings.phase2.max_parallel_starts,
            observed_at_ns,
        )?);
        let context_start_dispatches = work.release_after_graph_context_ids(
            &completion.graph_context_ids,
            self.settings.phase2.max_parallel_starts,
            observed_at_ns,
        )?;
        let start_dispatches = start_dispatches
            .into_iter()
            .chain(context_start_dispatches)
            .collect();

        work.commit(self);
        Ok(SupervisorFilesystemCheckCompletionDispatch {
            completion,
            start_dispatches,
        })
    }

    pub fn process_due_filesystem_check_timeout<P>(
        &mut self,
        operation_id: OperationId,
        now_ns: u64,
        controller: &mut P,
    ) -> Result<Option<SupervisorFilesystemCheckTimeoutDispatch>, SupervisorError>
    where
        P: crate::boundary::ProcessController + ?Sized,
    {
        let Some(deadline) = self
            .start
            .due_pre_start_check_deadlines(now_ns)
            .into_iter()
            .find(|deadline| deadline.operation_id == operation_id)
        else {
            return Ok(None);
        };
        let service = deadline.service.clone();
        let helper_cgroup_id = deadline.helper_cgroup_id.clone();

        let mut work = SupervisorWork::from_supervisor(self);
        let timeout = timeout_pre_start_check_helper(
            &mut StartExecutionContext {
                services: &mut work.services,
                operations: &mut work.operations,
                graph: &mut work.graph,
                jobs: &mut work.jobs,
                job_ids: &mut work.job_ids,
                start_store: &mut work.start,
                controller,
            },
            deadline,
            now_ns,
        )
        .map_err(SupervisorError::Start)?;
        if let Some(job_event) = &timeout.completion.job_event {
            work.queue_created_start_job(job_event);
        }
        let mut start_dispatches = apply_relationship_reactions_after_transitions(
            &mut work,
            &timeout.completion.service_transitions,
            now_ns,
            self.settings.phase2.max_parallel_starts,
        )?;
        start_dispatches.extend(work.release_after_graph_events(
            &timeout.completion.graph_events,
            self.settings.phase2.max_parallel_starts,
            now_ns,
        )?);
        let context_start_dispatches = work.release_after_graph_context_ids(
            &timeout.completion.graph_context_ids,
            self.settings.phase2.max_parallel_starts,
            now_ns,
        )?;
        let start_dispatches = start_dispatches
            .into_iter()
            .chain(context_start_dispatches)
            .collect();
        record_cgroup_cleanup(
            &mut work.cgroup_cleanup,
            service,
            helper_cgroup_id,
            CgroupCleanupKind::Helper,
            now_ns,
            self.settings.shutdown.post_kill_timeout_secs,
        );

        work.commit(self);
        Ok(Some(SupervisorFilesystemCheckTimeoutDispatch {
            timeout,
            start_dispatches,
        }))
    }
}
