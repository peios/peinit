use crate::boundary::BoundaryError;
use crate::execution::command::ExecutableCommandParseError;
use crate::execution::failure::StartFailureError;
use crate::execution::graph::GraphExecutionError;
use crate::ids::{JobId, OperationId};
use crate::job::{JobState, JobStoreError, JobType};
use crate::operation::store::OperationStoreError;
use crate::service::ServiceTableError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartExecutionError {
    JobIdAllocation(crate::ids::IdAllocationError),
    ServiceTable(ServiceTableError),
    OperationStore(OperationStoreError),
    JobStore(JobStoreError),
    HookJob(crate::job::ServiceHookJobBuildError),
    Graph(GraphExecutionError),
    Boundary(BoundaryError),
    StartFailure(StartFailureError),
    StartSatisfaction(crate::execution::satisfaction::StartSatisfactionError),
    InvalidExecStartPreCommand {
        service: String,
        command_index: usize,
        command: String,
        source: ExecutableCommandParseError,
    },
    InvalidExecStartPostCommand {
        service: String,
        command_index: usize,
        command: String,
        source: ExecutableCommandParseError,
    },
    EmptyExecStartPreSequence {
        service: String,
    },
    EmptyExecStartPostSequence {
        service: String,
    },
    NotPreExecHookJob {
        job_id: JobId,
        job_type: JobType,
    },
    NotPostExecHookJob {
        job_id: JobId,
        job_type: JobType,
    },
    MissingService {
        job_id: JobId,
    },
    MissingOperation {
        job_id: JobId,
        service: String,
    },
    MissingPreStartHookSequence {
        operation_id: OperationId,
    },
    MissingPostStartHookSequence {
        operation_id: OperationId,
    },
    MissingPrecheckedGraphStart {
        operation_id: OperationId,
    },
    UnknownPreStartCheckHelper {
        result_fd: i32,
    },
    MismatchedPreStartCheckReport {
        expected_operation_id: OperationId,
        actual_operation_id: OperationId,
    },
    UnexpectedFilesystemCheckContinuation {
        operation_id: OperationId,
    },
    NotTerminalJobEvent {
        job_id: JobId,
        state: JobState,
    },
}
