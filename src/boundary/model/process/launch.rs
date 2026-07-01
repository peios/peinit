use std::os::fd::OwnedFd;

use crate::boundary::BoundaryError;
use crate::job::JobRecord;
use crate::service::ServiceDefinition;

use super::launch_failure::{ProcessCleanupEvidence, ProcessPreExecError};
use super::token::TokenHandle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchedProcess {
    pub pid: u32,
    pub pidfd: i32,
    pub stdout_fd: Option<i32>,
    pub stderr_fd: Option<i32>,
    pub setup_status_fd: Option<i32>,
    pub cleanup_evidence: Vec<ProcessCleanupEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: String,
}

#[derive(Debug)]
pub struct ProcessLaunchSpec<'a> {
    pub job: &'a JobRecord,
    pub token: TokenHandle,
    pub environment: Vec<EnvironmentVariable>,
    pub inherited_fds: Vec<ProcessInheritedFd>,
    pub setup_timeout_secs: u64,
    pub output_pipe_buffer_bytes: usize,
}

impl ProcessLaunchSpec<'_> {
    pub fn environment_value(&self, name: &str) -> Option<&str> {
        self.environment
            .iter()
            .find(|variable| variable.name == name)
            .map(|variable| variable.value.as_str())
    }
}

#[derive(Debug)]
pub struct ProcessInheritedFd {
    pub name: String,
    pub fd: OwnedFd,
}

pub trait ProcessLauncher {
    fn provision_service_runtime_directories(
        &mut self,
        _service: &ServiceDefinition,
    ) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn launch_service(
        &mut self,
        spec: ProcessLaunchSpec<'_>,
    ) -> Result<LaunchedProcess, BoundaryError>;

    fn launch_job(
        &mut self,
        spec: ProcessLaunchSpec<'_>,
    ) -> Result<LaunchedProcess, BoundaryError> {
        self.launch_service(spec)
    }

    fn read_process_setup_status(&mut self, fd: i32) -> Result<ProcessSetupStatus, BoundaryError> {
        let _ = fd;
        Err(BoundaryError::Process(
            "process setup status reader unavailable".to_string(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessSetupStatus {
    Pending,
    ExecSucceeded,
    PreExecFailed(ProcessPreExecError),
    MalformedPreExec(String),
}

pub trait ProcessSetupStatusReader {
    fn read_process_setup_status(&mut self, fd: i32) -> Result<ProcessSetupStatus, BoundaryError>;
}
