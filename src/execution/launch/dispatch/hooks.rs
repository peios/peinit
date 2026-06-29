use crate::boundary::{ProcessLauncher, TokenProvider};
use crate::job::JobStore;
use crate::service::ServiceEnvironmentVariable;

use crate::execution::launch::dispatch::core::launch_created_job_result;
use crate::execution::launch::model::{
    LaunchCreatedJobError, LaunchCreatedJobRequest, LaunchCreatedJobResult,
};
use crate::execution::launch::target::LaunchTarget;

pub fn launch_created_reload_hook_job(
    jobs: &mut JobStore,
    token_provider: &mut (impl TokenProvider + ?Sized),
    process_launcher: &mut (impl ProcessLauncher + ?Sized),
    request: LaunchCreatedJobRequest,
) -> Result<LaunchCreatedJobResult, LaunchCreatedJobError> {
    launch_created_reload_hook_job_with_environment(
        jobs,
        token_provider,
        process_launcher,
        request,
        &[],
    )
}

pub fn launch_created_reload_hook_job_with_environment(
    jobs: &mut JobStore,
    token_provider: &mut (impl TokenProvider + ?Sized),
    process_launcher: &mut (impl ProcessLauncher + ?Sized),
    request: LaunchCreatedJobRequest,
    global_environment: &[ServiceEnvironmentVariable],
) -> Result<LaunchCreatedJobResult, LaunchCreatedJobError> {
    launch_created_job_result(
        jobs,
        token_provider,
        process_launcher,
        request,
        LaunchTarget::ReloadHook,
        global_environment,
        Vec::new(),
    )
}

pub fn launch_created_pre_exec_hook_job(
    jobs: &mut JobStore,
    token_provider: &mut (impl TokenProvider + ?Sized),
    process_launcher: &mut (impl ProcessLauncher + ?Sized),
    request: LaunchCreatedJobRequest,
) -> Result<LaunchCreatedJobResult, LaunchCreatedJobError> {
    launch_created_pre_exec_hook_job_with_environment(
        jobs,
        token_provider,
        process_launcher,
        request,
        &[],
    )
}

pub fn launch_created_pre_exec_hook_job_with_environment(
    jobs: &mut JobStore,
    token_provider: &mut (impl TokenProvider + ?Sized),
    process_launcher: &mut (impl ProcessLauncher + ?Sized),
    request: LaunchCreatedJobRequest,
    global_environment: &[ServiceEnvironmentVariable],
) -> Result<LaunchCreatedJobResult, LaunchCreatedJobError> {
    launch_created_job_result(
        jobs,
        token_provider,
        process_launcher,
        request,
        LaunchTarget::PreExecHook,
        global_environment,
        Vec::new(),
    )
}

pub fn launch_created_post_exec_hook_job_with_environment(
    jobs: &mut JobStore,
    token_provider: &mut (impl TokenProvider + ?Sized),
    process_launcher: &mut (impl ProcessLauncher + ?Sized),
    request: LaunchCreatedJobRequest,
    global_environment: &[ServiceEnvironmentVariable],
) -> Result<LaunchCreatedJobResult, LaunchCreatedJobError> {
    launch_created_job_result(
        jobs,
        token_provider,
        process_launcher,
        request,
        LaunchTarget::PostExecHook,
        global_environment,
        Vec::new(),
    )
}

pub fn launch_created_health_check_job_with_environment(
    jobs: &mut JobStore,
    token_provider: &mut (impl TokenProvider + ?Sized),
    process_launcher: &mut (impl ProcessLauncher + ?Sized),
    request: LaunchCreatedJobRequest,
    global_environment: &[ServiceEnvironmentVariable],
) -> Result<LaunchCreatedJobResult, LaunchCreatedJobError> {
    launch_created_job_result(
        jobs,
        token_provider,
        process_launcher,
        request,
        LaunchTarget::HealthCheck,
        global_environment,
        Vec::new(),
    )
}
