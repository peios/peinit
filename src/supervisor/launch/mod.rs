mod control;
mod failure;
mod fd_inheritance;
mod hooks;
mod request;
mod service;

pub(in crate::supervisor) use failure::{
    apply_pre_start_hook_launch_failure, apply_service_launch_failure,
};
pub(in crate::supervisor) use service::apply_started_service_launch;

use crate::boundary::BoundaryError;
use crate::execution::launch::{LaunchCreatedJobError, PendingLaunchSetup};
use crate::supervisor::dispatch::SupervisorPendingProcessSetupDispatch;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

pub(in crate::supervisor) fn record_pending_process_setup(
    work: &mut SupervisorWork,
    setup: PendingLaunchSetup,
) -> Result<SupervisorPendingProcessSetupDispatch, SupervisorError> {
    let job = work
        .jobs
        .get(setup.job_id)
        .ok_or(crate::job::JobStoreError::UnknownJob { id: setup.job_id })
        .map_err(SupervisorError::JobStore)?;
    let job_type = job.job_type;
    let job_id = setup.job_id;
    let setup_status_fd = setup.setup_status_fd().ok_or_else(|| {
        SupervisorError::Launch(LaunchCreatedJobError::Boundary(BoundaryError::Process(
            format!("pending launch for job {job_id} has no setup status fd"),
        )))
    })?;
    work.record_pending_process_setup(setup);
    Ok(SupervisorPendingProcessSetupDispatch {
        job_id,
        job_type,
        setup_status_fd,
    })
}
