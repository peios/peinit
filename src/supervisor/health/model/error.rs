use crate::ids::JobId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthCheckError {
    MissingService {
        job_id: JobId,
    },
    UnknownService {
        service: String,
    },
    MissingMainJob {
        service: String,
    },
    InvalidCommand {
        service: String,
        command: String,
        source: crate::execution::command::ExecutableCommandParseError,
    },
    JobIdAllocation(crate::ids::IdAllocationError),
    JobBuild(crate::job::ServiceHookJobBuildError),
    JobStore(crate::job::JobStoreError),
    ServiceTable(crate::service::ServiceTableError),
    Launch(crate::execution::launch::LaunchCreatedJobError),
    Boundary(crate::boundary::BoundaryError),
    NotHealthCheckJob {
        job_id: JobId,
        job_type: crate::job::JobType,
    },
    NotTerminalJobEvent {
        job_id: JobId,
        state: crate::job::JobState,
    },
}
