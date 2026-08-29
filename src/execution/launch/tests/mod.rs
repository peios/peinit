mod failure;
mod success;

use crate::boundary::{
    BoundaryError, LaunchedProcess, ProcessLaunchSpec, ProcessLauncher, TokenHandle, TokenProvider,
};
use crate::execution::launch::{LaunchCreatedJobRequest, NOTIFY_SOCKET, PATH};
use crate::ids::{JobId, JobIdAllocator, OperationId, OperationIdAllocator};
use crate::job::{JobRecord, JobStore, ServiceMainJobSpec};
use crate::logging::DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES;
use crate::security::TokenSummary;
use crate::service::{ErrorControl, ServiceDefinition, ServiceEnvironmentVariable};

const CREATED_AT_NS: u64 = 1_000;
const LAUNCHED_AT_NS: u64 = 1_100;
const SETUP_TIMEOUT_SECS: u64 = 45;
const TEST_NOTIFY_SOCKET: &str = "/run/test/notify.sock";

struct FakeTokenProvider {
    result: Result<TokenHandle, BoundaryError>,
    observed_jobs: Vec<JobId>,
}

impl FakeTokenProvider {
    fn success() -> Self {
        Self {
            result: Ok(TokenHandle {
                fd: 8,
                identity: "SYSTEM".to_string(),
                summary: TokenSummary::new(
                    "SYSTEM",
                    "S-1-5-18",
                    vec!["S-1-5-80-1-2-3-4-5".to_string()],
                    vec!["SeChangeNotifyPrivilege".to_string()],
                    vec!["SeChangeNotifyPrivilege".to_string()],
                ),
            }),
            observed_jobs: Vec::new(),
        }
    }

    fn failure(error: BoundaryError) -> Self {
        Self {
            result: Err(error),
            observed_jobs: Vec::new(),
        }
    }
}

impl TokenProvider for FakeTokenProvider {
    fn materialize_service_token(&mut self, job: &JobRecord) -> Result<TokenHandle, BoundaryError> {
        self.observed_jobs.push(job.id);
        self.result.clone()
    }
}

struct FakeProcessLauncher {
    result: Result<LaunchedProcess, BoundaryError>,
    observed: Vec<ObservedLaunch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedLaunch {
    job_id: JobId,
    token_fd: i32,
    notify_socket: Option<String>,
    path: Option<String>,
    app_mode: Option<String>,
    working_directory: String,
    limit_nofile: Option<u64>,
    limit_core: Option<u64>,
    oom_score_adj: i32,
    setup_timeout_secs: u64,
    output_pipe_buffer_bytes: usize,
}

impl FakeProcessLauncher {
    fn success() -> Self {
        Self {
            result: Ok(LaunchedProcess {
                pid: 1234,
                pidfd: 9,
                stdout_fd: None,
                stderr_fd: None,
                setup_status_fd: None,
                cleanup_evidence: Vec::new(),
            }),
            observed: Vec::new(),
        }
    }

    fn failure(error: BoundaryError) -> Self {
        Self {
            result: Err(error),
            observed: Vec::new(),
        }
    }
}

impl ProcessLauncher for FakeProcessLauncher {
    fn launch_service(
        &mut self,
        spec: ProcessLaunchSpec<'_>,
    ) -> Result<LaunchedProcess, BoundaryError> {
        self.observed.push(ObservedLaunch {
            job_id: spec.job.id,
            token_fd: spec.token.fd,
            notify_socket: spec
                .environment_value(NOTIFY_SOCKET)
                .map(ToString::to_string),
            path: spec.environment_value(PATH).map(ToString::to_string),
            app_mode: spec.environment_value("APP_MODE").map(ToString::to_string),
            working_directory: spec.job.working_directory.clone(),
            limit_nofile: spec.job.limit_nofile,
            limit_core: spec.job.limit_core,
            oom_score_adj: spec.job.oom_score_adj,
            setup_timeout_secs: spec.setup_timeout_secs,
            output_pipe_buffer_bytes: spec.output_pipe_buffer_bytes,
        });
        self.result.clone()
    }
}

fn launch_request(job_id: JobId) -> LaunchCreatedJobRequest {
    LaunchCreatedJobRequest {
        job_id,
        launched_at_ns: LAUNCHED_AT_NS,
        notify_socket_path: TEST_NOTIFY_SOCKET.to_string(),
        setup_timeout_secs: SETUP_TIMEOUT_SECS,
        output_pipe_buffer_bytes: DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES,
        token_source: crate::execution::launch::LaunchTokenSource::ServiceIdentity,
    }
}

fn job_store_with_service_main(job_id: JobId, operation_id: OperationId) -> JobStore {
    let mut jobs = JobStore::new();
    jobs.create_job(service_main_job(job_id, operation_id))
        .expect("create job");
    jobs
}

fn service_main_job(job_id: JobId, operation_id: OperationId) -> JobRecord {
    let mut service = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    service.environment = vec![
        ServiceEnvironmentVariable {
            name: "APP_MODE".to_string(),
            value: "prod".to_string(),
        },
        ServiceEnvironmentVariable {
            name: PATH.to_string(),
            value: "/service/bin".to_string(),
        },
    ];
    service.working_directory = "/srv/app".to_string();
    service.limit_nofile = Some(4096);
    service.limit_core = Some(0);
    service.error_control = ErrorControl::Critical;
    JobRecord::new_service_main(
        job_id,
        ServiceMainJobSpec {
            service: &service,
            resolved_identity: "SYSTEM".to_string(),
            token_summary: token_summary(),
            activation_generation: 1,
            cgroup_generation: 0,
            operation_id,
            created_at_ns: CREATED_AT_NS,
        },
    )
}

fn token_summary() -> TokenSummary {
    TokenSummary::requested_identity("SYSTEM")
}

fn ids() -> (JobId, OperationId) {
    let job_id = JobIdAllocator::new()
        .allocate_batch(1, CREATED_AT_NS)
        .expect("job id")[0];
    let operation_id = OperationIdAllocator::new()
        .allocate_batch(1, CREATED_AT_NS)
        .expect("operation id")[0];
    (job_id, operation_id)
}
