use std::os::fd::{AsRawFd, IntoRawFd};

use super::{
    BoundaryError, FilesystemCheckHelperLauncher, FilesystemCheckHelperReader,
    FilesystemCheckHelperRequest, FilesystemCheckReport, FilesystemCheckResult,
    LaunchedFilesystemCheckHelper,
};

mod cgroup;
mod clone3;
mod pipe;
mod report;
mod run_child;

use cgroup::{create_cgroup, open_cgroup_directory};
use clone3::{CloneResult, clone_into_cgroup};
use pipe::{ReadOutcome, create_result_pipe, read_fd};
use report::decode_satisfied_bytes;
use run_child::run_child_checks;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LinuxFilesystemCheckHelper;

impl LinuxFilesystemCheckHelper {
    pub fn new() -> Self {
        Self
    }
}

impl FilesystemCheckHelperLauncher for LinuxFilesystemCheckHelper {
    fn launch_filesystem_check_helper(
        &mut self,
        request: FilesystemCheckHelperRequest,
    ) -> Result<LaunchedFilesystemCheckHelper, BoundaryError> {
        launch_linux_filesystem_check_helper(request)
    }
}

impl FilesystemCheckHelperReader for LinuxFilesystemCheckHelper {
    fn read_filesystem_check_report(
        &mut self,
        helper: &LaunchedFilesystemCheckHelper,
    ) -> Result<Option<FilesystemCheckReport>, BoundaryError> {
        read_linux_filesystem_check_report(helper)
    }
}

fn launch_linux_filesystem_check_helper(
    request: FilesystemCheckHelperRequest,
) -> Result<LaunchedFilesystemCheckHelper, BoundaryError> {
    create_cgroup(&request.cgroup_id)?;
    let cgroup = open_cgroup_directory(&request.cgroup_id)?;
    let (read, write) = create_result_pipe()?;
    let child = clone_into_cgroup(cgroup.as_raw_fd())?;
    match child {
        CloneResult::Child => {
            drop(read);
            run_child_checks(&request.checks, write.as_raw_fd());
        }
        CloneResult::Parent { pid, pidfd } => {
            drop(write);
            Ok(LaunchedFilesystemCheckHelper {
                service: request.service,
                operation_id: request.operation_id,
                checks: request.checks,
                pid,
                pidfd: pidfd.into_raw_fd(),
                result_fd: read.into_raw_fd(),
                cgroup_id: request.cgroup_id,
            })
        }
    }
}

fn read_linux_filesystem_check_report(
    helper: &LaunchedFilesystemCheckHelper,
) -> Result<Option<FilesystemCheckReport>, BoundaryError> {
    let expected_len = 4usize.saturating_add(helper.checks.len());
    let mut bytes = vec![0; expected_len];
    let len = match read_fd(helper.result_fd, &mut bytes)? {
        ReadOutcome::Bytes(len) => len,
        ReadOutcome::WouldBlock => return Ok(None),
    };
    let satisfied = decode_satisfied_bytes(&bytes[..len], helper.checks.len());
    let results = helper
        .checks
        .iter()
        .cloned()
        .zip(satisfied)
        .map(|(check, satisfied)| FilesystemCheckResult { check, satisfied })
        .collect();
    Ok(Some(FilesystemCheckReport {
        service: helper.service.clone(),
        operation_id: helper.operation_id,
        results,
    }))
}
