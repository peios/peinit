use std::os::fd::RawFd;

use crate::boundary::{BoundaryError, ProcessInheritedFd, ProcessLaunchError};
use crate::execution::launch::LaunchCreatedJobError;
use crate::job::JobType;

use super::super::work::SupervisorWork;

pub(super) fn inherited_fds_for_job(
    work: &SupervisorWork,
    job_id: crate::ids::JobId,
) -> Result<Vec<ProcessInheritedFd>, LaunchCreatedJobError> {
    let Some(job) = work.jobs.get(job_id) else {
        return Ok(Vec::new());
    };
    if job.job_type != JobType::ServiceMain {
        return Ok(Vec::new());
    }
    let Some(service) = job.service.as_deref() else {
        return Ok(Vec::new());
    };
    let Some(store) = work.fd_store.service(service) else {
        return Ok(Vec::new());
    };

    let min_fd = inherited_fd_duplicate_min(store.len())?;
    store
        .entries()
        .iter()
        .map(|entry| {
            let fd = entry.fd.duplicate_min(min_fd).map_err(|error| {
                launch_parent_setup(format!(
                    "duplicate stored fd '{}' for service '{service}' failed: {error}",
                    entry.name
                ))
            })?;
            Ok(ProcessInheritedFd {
                name: entry.name.clone(),
                fd,
            })
        })
        .collect()
}

fn inherited_fd_duplicate_min(count: usize) -> Result<RawFd, LaunchCreatedJobError> {
    let count = RawFd::try_from(count)
        .map_err(|_| launch_parent_setup("stored fd count exceeds RawFd range"))?;
    3i32.checked_add(count)
        .ok_or_else(|| launch_parent_setup("stored fd count exceeds inherited fd range"))
}

fn launch_parent_setup(message: impl Into<String>) -> LaunchCreatedJobError {
    LaunchCreatedJobError::Boundary(BoundaryError::ProcessLaunch(
        ProcessLaunchError::parent_setup(message),
    ))
}
