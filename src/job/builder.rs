use crate::boot::phase2::PreparedStart;
use crate::ids::{JobId, OperationId};
use crate::security::TokenSummary;
use crate::service::{ErrorControl, ServiceDefinition};

use super::cgroup::{ServiceCgroupKind, service_job_cgroup_path, submitted_job_cgroup_path};
use super::model::{JobRecord, JobState, JobType};

#[derive(Debug, Clone)]
pub struct ServiceMainJobSpec<'a> {
    pub service: &'a ServiceDefinition,
    pub resolved_identity: String,
    pub token_summary: TokenSummary,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub operation_id: OperationId,
    pub created_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedJobSpec {
    pub identity_user_sid: String,
    pub token_summary: TokenSummary,
    pub image_path: String,
    pub arguments: Vec<String>,
    pub environment: Vec<crate::service::ServiceEnvironmentVariable>,
    pub working_directory: String,
    pub created_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceMainJobBuildError {
    ServiceMismatch {
        start_service: String,
        definition_service: String,
    },
}

impl JobRecord {
    pub fn new_service_main(id: JobId, spec: ServiceMainJobSpec<'_>) -> Self {
        let ServiceMainJobSpec {
            service,
            resolved_identity,
            token_summary,
            activation_generation,
            cgroup_generation,
            operation_id,
            created_at_ns,
        } = spec;
        Self {
            id,
            service: Some(service.name.clone()),
            job_type: JobType::ServiceMain,
            hook_index: None,
            state: JobState::Created,
            pid: None,
            pidfd: None,
            resolved_identity,
            token_summary,
            required_privileges: service.required_privileges.clone(),
            image_path: service.image_path.clone(),
            arguments: service.arguments.clone(),
            environment: service.environment.clone(),
            working_directory: service.working_directory.clone(),
            limit_nofile: service.limit_nofile,
            limit_core: service.limit_core,
            oom_score_adj: oom_score_adj(service.error_control),
            created_at_ns,
            started_at_ns: None,
            ended_at_ns: None,
            exit_code: None,
            exit_signal: None,
            failure_cause: None,
            cgroup_id: service_job_cgroup_path(
                &service.name,
                cgroup_generation,
                ServiceCgroupKind::Main,
            ),
            activation_generation,
            cgroup_generation,
            operation_id: Some(operation_id),
            console_path: service.console_path.clone(),
        }
    }

    /// The record for a job submitted on the jobs socket.
    ///
    /// `resolved_identity` is the job identity's user SID: there is no
    /// identity *string* to resolve, because a submitted job's identity is a
    /// token the kernel conveyed, never a name.
    pub fn new_submitted(id: JobId, spec: SubmittedJobSpec) -> Self {
        let SubmittedJobSpec {
            identity_user_sid,
            token_summary,
            image_path,
            arguments,
            environment,
            working_directory,
            created_at_ns,
        } = spec;
        Self {
            id,
            service: None,
            job_type: JobType::Submitted,
            hook_index: None,
            state: JobState::Created,
            pid: None,
            pidfd: None,
            resolved_identity: identity_user_sid,
            token_summary,
            required_privileges: Vec::new(),
            image_path,
            arguments,
            environment,
            working_directory,
            limit_nofile: None,
            limit_core: None,
            oom_score_adj: 0,
            created_at_ns,
            started_at_ns: None,
            ended_at_ns: None,
            exit_code: None,
            exit_signal: None,
            failure_cause: None,
            cgroup_id: submitted_job_cgroup_path(id),
            activation_generation: 0,
            cgroup_generation: 0,
            operation_id: None,
            console_path: None,
        }
    }
}

fn oom_score_adj(error_control: ErrorControl) -> i32 {
    match error_control {
        ErrorControl::Critical => -1000,
        ErrorControl::Normal => 0,
    }
}

pub fn service_main_job_from_phase2_start(
    start: &PreparedStart,
    service: &ServiceDefinition,
    token_summary: TokenSummary,
    activation_generation: u64,
    cgroup_generation: u64,
    created_at_ns: u64,
) -> Result<JobRecord, ServiceMainJobBuildError> {
    if start.service != service.name {
        return Err(ServiceMainJobBuildError::ServiceMismatch {
            start_service: start.service.clone(),
            definition_service: service.name.clone(),
        });
    }
    Ok(JobRecord::new_service_main(
        start.job_id,
        ServiceMainJobSpec {
            service,
            resolved_identity: start.identity.clone(),
            token_summary,
            activation_generation,
            cgroup_generation,
            operation_id: start.operation_id,
            created_at_ns,
        },
    ))
}
