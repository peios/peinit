mod authd;
mod cgroup;
mod command;
mod fd;
mod process;
mod system_token;
mod token_info;

use std::os::fd::IntoRawFd;

use self::authd::AuthdTokenClient;
use super::{
    BoundaryError, LaunchedProcess, ProcessLaunchSpec, ProcessLauncher, ProcessSetupStatus,
    ProcessSetupStatusReader, TokenHandle, TokenProvider,
};
use crate::job::JobRecord;
use crate::security::is_system_identity;
use crate::service::ServiceDefinition;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LinuxSystemTokenProvider {
    authd: authd::HardcodedAuthdTokenClient,
}

impl LinuxSystemTokenProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TokenProvider for LinuxSystemTokenProvider {
    fn materialize_service_token(&mut self, job: &JobRecord) -> Result<TokenHandle, BoundaryError> {
        let identity = job.resolved_identity.clone();
        let token = match token_materialization_request(job)? {
            TokenMaterializationRequest::System { service } => {
                system_token::create_system_token(&service)?
            }
            TokenMaterializationRequest::Authd(request) => {
                self.authd.request_service_token(request)?
            }
        };
        token_info::apply_required_privileges(&token, &job.required_privileges)?;
        let summary = token_info::summarize_token(&identity, &token)?;
        Ok(TokenHandle {
            fd: token.into_raw_fd(),
            identity,
            summary,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenMaterializationRequest {
    System { service: String },
    Authd(authd::AuthdTokenRequest),
}

fn token_materialization_request(
    job: &JobRecord,
) -> Result<TokenMaterializationRequest, BoundaryError> {
    let service = job.service.clone().ok_or_else(|| {
        BoundaryError::Token(format!("job {} has no service identity context", job.id))
    })?;
    if is_system_identity(&job.resolved_identity) {
        return Ok(TokenMaterializationRequest::System { service });
    }
    Ok(TokenMaterializationRequest::Authd(
        authd::AuthdTokenRequest {
            identity: job.resolved_identity.clone(),
            service,
        },
    ))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LinuxProcessLauncher;

impl LinuxProcessLauncher {
    pub fn new() -> Self {
        Self
    }
}

impl ProcessLauncher for LinuxProcessLauncher {
    fn provision_service_runtime_directories(
        &mut self,
        service: &ServiceDefinition,
    ) -> Result<(), BoundaryError> {
        crate::boundary::provision_linux_service_runtime_directories(service).map_err(|error| {
            BoundaryError::Process(format!(
                "provision runtime directories for {} failed: {error}",
                service.name
            ))
        })
    }

    fn launch_service(
        &mut self,
        spec: ProcessLaunchSpec<'_>,
    ) -> Result<LaunchedProcess, BoundaryError> {
        process::launch_linux_process(spec)
    }

    fn read_process_setup_status(&mut self, fd: i32) -> Result<ProcessSetupStatus, BoundaryError> {
        process::read_linux_process_setup_status(fd)
    }
}

impl ProcessSetupStatusReader for LinuxProcessLauncher {
    fn read_process_setup_status(&mut self, fd: i32) -> Result<ProcessSetupStatus, BoundaryError> {
        process::read_linux_process_setup_status(fd)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::{TokenMaterializationRequest, authd, token_materialization_request};
    use crate::job::{JobRecord, ServiceMainJobSpec};
    use crate::security::TokenSummary;
    use crate::service::ServiceDefinition;

    pub(super) fn test_job() -> JobRecord {
        let mut service = ServiceDefinition::simple_system_boot("app", "/sbin/app");
        service.arguments = vec!["--foreground".to_string()];
        JobRecord::new_service_main(
            crate::ids::JobIdAllocator::new()
                .allocate_batch(1, 1)
                .expect("job id")[0],
            ServiceMainJobSpec {
                service: &service,
                resolved_identity: "SYSTEM".to_string(),
                token_summary: TokenSummary::requested_identity("SYSTEM"),
                activation_generation: 1,
                cgroup_generation: 0,
                operation_id: crate::ids::OperationIdAllocator::new()
                    .allocate_batch(1, 1)
                    .expect("operation id")[0],
                created_at_ns: 1,
            },
        )
    }

    #[test]
    fn system_identity_uses_local_system_token_materialization() {
        let job = test_job();

        assert_eq!(
            token_materialization_request(&job).expect("request"),
            TokenMaterializationRequest::System {
                service: "app".to_string()
            },
        );
    }

    #[test]
    fn non_system_identities_request_authd_token_for_resolved_service_identity() {
        for identity in ["LocalService", "NetworkService", "S-1-5-21-42"] {
            let mut job = test_job();
            job.resolved_identity = identity.to_string();

            assert_eq!(
                token_materialization_request(&job).expect("request"),
                TokenMaterializationRequest::Authd(authd::AuthdTokenRequest {
                    identity: identity.to_string(),
                    service: "app".to_string(),
                }),
            );
        }
    }

    #[test]
    fn token_materialization_rejects_job_without_service_identity_context() {
        let mut job = test_job();
        job.service = None;

        let error = token_materialization_request(&job).expect_err("missing service");

        assert!(matches!(
            error,
            crate::boundary::BoundaryError::Token(message)
                if message.contains("has no service identity context")
        ));
    }
}
