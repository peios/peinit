use crate::boundary::{
    ProcessInheritedFd, ProcessLaunchSpec, ProcessLauncher, TokenHandle, TokenProvider,
};
use crate::job::{JobRecord, JobStore, ProcessHandle};
use crate::service::ServiceEnvironmentVariable;

use crate::execution::launch::environment::build_launch_environment_with_inherited_fds;
use crate::execution::launch::model::{
    LaunchCreatedJobDispatch, LaunchCreatedJobError, LaunchCreatedJobRequest,
    LaunchCreatedJobResult, LaunchTokenSource, PendingLaunchSetup,
};
use crate::execution::launch::target::LaunchTarget;

pub(super) fn launch_created_job_result(
    jobs: &mut JobStore,
    token_provider: &mut (impl TokenProvider + ?Sized),
    process_launcher: &mut (impl ProcessLauncher + ?Sized),
    request: LaunchCreatedJobRequest,
    target: LaunchTarget,
    global_environment: &[ServiceEnvironmentVariable],
    inherited_fds: Vec<ProcessInheritedFd>,
) -> Result<LaunchCreatedJobResult, LaunchCreatedJobError> {
    let job = get_job(jobs, request.job_id)?;
    target.validate(&job, request.launched_at_ns)?;

    let token = match request.token_source {
        LaunchTokenSource::ServiceIdentity => token_provider.materialize_service_token(&job),
        LaunchTokenSource::Prepared { token_fd } => {
            token_provider.materialize_prepared_token(&job, token_fd)
        }
    }
    .map_err(LaunchCreatedJobError::Boundary)?;
    let token_summary = token.summary.clone();
    let process = target
        .launch(
            process_launcher,
            launch_spec(
                &job,
                token,
                &request.notify_socket_path,
                global_environment,
                inherited_fds,
                request.setup_timeout_secs,
                request.output_pipe_buffer_bytes,
            ),
        )
        .map_err(LaunchCreatedJobError::Boundary)?;

    if process.setup_status_fd.is_some() {
        return Ok(LaunchCreatedJobResult::PendingSetup(PendingLaunchSetup {
            job_id: request.job_id,
            process,
            token_summary,
            launched_at_ns: request.launched_at_ns,
            setup_deadline_ns: setup_deadline_ns(
                request.launched_at_ns,
                request.setup_timeout_secs,
            ),
        }));
    }

    let job_event = jobs
        .start_job_with_token_summary(
            request.job_id,
            ProcessHandle {
                pid: process.pid,
                pidfd: process.pidfd,
            },
            token_summary,
            request.launched_at_ns,
        )
        .map_err(LaunchCreatedJobError::JobStore)?;

    Ok(LaunchCreatedJobResult::Started(Box::new(
        LaunchCreatedJobDispatch {
            job_id: request.job_id,
            process,
            job_event,
        },
    )))
}

fn setup_deadline_ns(launched_at_ns: u64, setup_timeout_secs: u64) -> u64 {
    launched_at_ns.saturating_add(setup_timeout_secs.saturating_mul(1_000_000_000))
}

fn get_job(jobs: &JobStore, job_id: crate::ids::JobId) -> Result<JobRecord, LaunchCreatedJobError> {
    jobs.get(job_id)
        .cloned()
        .ok_or(LaunchCreatedJobError::JobStore(
            crate::job::JobStoreError::UnknownJob { id: job_id },
        ))
}

fn launch_spec<'a>(
    job: &'a JobRecord,
    token: TokenHandle,
    notify_socket_path: &str,
    global_environment: &[ServiceEnvironmentVariable],
    inherited_fds: Vec<ProcessInheritedFd>,
    setup_timeout_secs: u64,
    output_pipe_buffer_bytes: usize,
) -> ProcessLaunchSpec<'a> {
    ProcessLaunchSpec {
        job,
        token,
        environment: build_launch_environment_with_inherited_fds(
            job,
            notify_socket_path,
            global_environment,
            &inherited_fds,
        ),
        inherited_fds,
        setup_timeout_secs,
        output_pipe_buffer_bytes,
    }
}
