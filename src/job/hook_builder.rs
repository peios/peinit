use crate::ids::{JobId, OperationId};
use crate::security::TokenSummary;
use crate::service::ServiceDefinition;

use super::cgroup::{ServiceCgroupKind, service_job_cgroup_path};
use super::model::{JobRecord, JobState, JobType};

#[derive(Debug, Clone)]
pub struct ServiceHookJobSpec<'a> {
    pub service: &'a ServiceDefinition,
    pub argv: Vec<String>,
    pub hook_index: Option<usize>,
    pub resolved_identity: String,
    pub token_summary: TokenSummary,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub operation_id: OperationId,
    pub created_at_ns: u64,
}

#[derive(Debug, Clone)]
pub struct ServiceHealthCheckJobSpec<'a> {
    pub service: &'a ServiceDefinition,
    pub argv: Vec<String>,
    pub resolved_identity: String,
    pub token_summary: TokenSummary,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub created_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceHookJobBuildError {
    EmptyArgv { service: String, job_type: JobType },
}

impl JobRecord {
    pub fn new_reload_hook(
        id: JobId,
        spec: ServiceHookJobSpec<'_>,
    ) -> Result<Self, ServiceHookJobBuildError> {
        new_service_hook_job(id, spec, JobType::ReloadHook)
    }

    pub fn new_pre_exec_hook(
        id: JobId,
        spec: ServiceHookJobSpec<'_>,
    ) -> Result<Self, ServiceHookJobBuildError> {
        new_service_hook_job(id, spec, JobType::PreExecHook)
    }

    pub fn new_post_exec_hook(
        id: JobId,
        spec: ServiceHookJobSpec<'_>,
    ) -> Result<Self, ServiceHookJobBuildError> {
        new_service_hook_job(id, spec, JobType::PostExecHook)
    }

    pub fn new_health_check(
        id: JobId,
        spec: ServiceHealthCheckJobSpec<'_>,
    ) -> Result<Self, ServiceHookJobBuildError> {
        let ServiceHealthCheckJobSpec {
            service,
            argv,
            resolved_identity,
            token_summary,
            activation_generation,
            cgroup_generation,
            created_at_ns,
        } = spec;
        let mut argv = argv.into_iter();
        let Some(image_path) = argv.next() else {
            return Err(ServiceHookJobBuildError::EmptyArgv {
                service: service.name.clone(),
                job_type: JobType::HealthCheck,
            });
        };

        Ok(JobRecord {
            id,
            service: Some(service.name.clone()),
            job_type: JobType::HealthCheck,
            hook_index: None,
            state: JobState::Created,
            pid: None,
            pidfd: None,
            resolved_identity,
            token_summary,
            required_privileges: service.required_privileges.clone(),
            image_path,
            arguments: argv.collect(),
            environment: service.environment.clone(),
            working_directory: service.working_directory.clone(),
            limit_nofile: service.limit_nofile,
            limit_core: service.limit_core,
            oom_score_adj: 0,
            created_at_ns,
            started_at_ns: None,
            ended_at_ns: None,
            exit_code: None,
            exit_signal: None,
            failure_cause: None,
            cgroup_id: service_job_cgroup_path(
                &service.name,
                cgroup_generation,
                ServiceCgroupKind::Health,
            ),
            activation_generation,
            cgroup_generation,
            operation_id: None,
            console_path: None,
        })
    }
}

fn new_service_hook_job(
    id: JobId,
    spec: ServiceHookJobSpec<'_>,
    job_type: JobType,
) -> Result<JobRecord, ServiceHookJobBuildError> {
    let ServiceHookJobSpec {
        service,
        argv,
        hook_index,
        resolved_identity,
        token_summary,
        activation_generation,
        cgroup_generation,
        operation_id,
        created_at_ns,
    } = spec;
    let mut argv = argv.into_iter();
    let Some(image_path) = argv.next() else {
        return Err(ServiceHookJobBuildError::EmptyArgv {
            service: service.name.clone(),
            job_type,
        });
    };

    Ok(JobRecord {
        id,
        service: Some(service.name.clone()),
        job_type,
        hook_index,
        state: JobState::Created,
        pid: None,
        pidfd: None,
        resolved_identity,
        token_summary,
        required_privileges: service.required_privileges.clone(),
        image_path,
        arguments: argv.collect(),
        environment: service.environment.clone(),
        working_directory: service.working_directory.clone(),
        limit_nofile: service.limit_nofile,
        limit_core: service.limit_core,
        oom_score_adj: 0,
        created_at_ns,
        started_at_ns: None,
        ended_at_ns: None,
        exit_code: None,
        exit_signal: None,
        failure_cause: None,
        cgroup_id: service_job_cgroup_path(
            &service.name,
            cgroup_generation,
            ServiceCgroupKind::Hooks,
        ),
        activation_generation,
        cgroup_generation,
        operation_id: Some(operation_id),
        console_path: None,
    })
}
