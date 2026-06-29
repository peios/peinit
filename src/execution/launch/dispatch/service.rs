use crate::boundary::{ProcessInheritedFd, ProcessLauncher, TokenProvider};
use crate::job::JobStore;
use crate::service::ServiceEnvironmentVariable;

use crate::execution::launch::dispatch::core::launch_created_job_result;
use crate::execution::launch::model::{
    LaunchCreatedJobError, LaunchCreatedJobRequest, LaunchCreatedJobResult,
};
use crate::execution::launch::target::LaunchTarget;

pub fn launch_created_service_main_job(
    jobs: &mut JobStore,
    token_provider: &mut (impl TokenProvider + ?Sized),
    process_launcher: &mut (impl ProcessLauncher + ?Sized),
    request: LaunchCreatedJobRequest,
) -> Result<LaunchCreatedJobResult, LaunchCreatedJobError> {
    launch_created_service_main_job_with_environment(
        jobs,
        token_provider,
        process_launcher,
        request,
        &[],
        Vec::new(),
    )
}

pub fn launch_created_service_main_job_with_inherited_fds(
    jobs: &mut JobStore,
    token_provider: &mut (impl TokenProvider + ?Sized),
    process_launcher: &mut (impl ProcessLauncher + ?Sized),
    request: LaunchCreatedJobRequest,
    inherited_fds: Vec<ProcessInheritedFd>,
) -> Result<LaunchCreatedJobResult, LaunchCreatedJobError> {
    launch_created_service_main_job_with_environment(
        jobs,
        token_provider,
        process_launcher,
        request,
        &[],
        inherited_fds,
    )
}

pub fn launch_created_service_main_job_with_environment(
    jobs: &mut JobStore,
    token_provider: &mut (impl TokenProvider + ?Sized),
    process_launcher: &mut (impl ProcessLauncher + ?Sized),
    request: LaunchCreatedJobRequest,
    global_environment: &[ServiceEnvironmentVariable],
    inherited_fds: Vec<ProcessInheritedFd>,
) -> Result<LaunchCreatedJobResult, LaunchCreatedJobError> {
    launch_created_job_result(
        jobs,
        token_provider,
        process_launcher,
        request,
        LaunchTarget::ServiceMain,
        global_environment,
        inherited_fds,
    )
}
