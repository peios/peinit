use crate::ids::OperationId;
use crate::service::ServiceCheck;

use super::error::BoundaryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemCheckHelperRequest {
    pub service: String,
    pub operation_id: OperationId,
    pub cgroup_id: String,
    pub checks: Vec<ServiceCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchedFilesystemCheckHelper {
    pub service: String,
    pub operation_id: OperationId,
    pub checks: Vec<ServiceCheck>,
    pub pid: u32,
    pub pidfd: i32,
    pub result_fd: i32,
    pub cgroup_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemCheckReport {
    pub service: String,
    pub operation_id: OperationId,
    pub results: Vec<FilesystemCheckResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemCheckResult {
    pub check: ServiceCheck,
    pub satisfied: bool,
}

pub trait FilesystemCheckHelperLauncher {
    fn launch_filesystem_check_helper(
        &mut self,
        request: FilesystemCheckHelperRequest,
    ) -> Result<LaunchedFilesystemCheckHelper, BoundaryError>;
}

pub trait FilesystemCheckHelperReader {
    fn read_filesystem_check_report(
        &mut self,
        helper: &LaunchedFilesystemCheckHelper,
    ) -> Result<Option<FilesystemCheckReport>, BoundaryError>;
}
