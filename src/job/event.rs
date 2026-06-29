use crate::ids::{JobId, OperationId};
use crate::security::TokenSummary;

use super::model::{JobRecord, JobState, JobType};
use super::store::JobStoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobEvent {
    pub job_id: JobId,
    pub service: Option<String>,
    pub job_type: JobType,
    pub hook_index: Option<usize>,
    pub state: JobState,
    pub pid: Option<u32>,
    pub pidfd: Option<i32>,
    pub resolved_identity: String,
    pub operation_id: Option<OperationId>,
    pub token_summary: TokenSummary,
    pub image_path: String,
    pub arguments: Vec<String>,
    pub created_at_ns: u64,
    pub started_at_ns: Option<u64>,
    pub ended_at_ns: Option<u64>,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<i32>,
    pub failure_cause: Option<String>,
    pub cgroup_id: String,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub detail: JobEventDetail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEventDetail {
    Created {
        image_path: String,
        identity: String,
        operation_id: Option<OperationId>,
    },
    Started {
        started_at_ns: u64,
        pid: u32,
        cgroup_id: String,
    },
    Ended {
        ended_at_ns: u64,
        duration_ns: u64,
        exit_code: Option<i32>,
        exit_signal: Option<i32>,
        failure_cause: Option<String>,
    },
}

impl JobEvent {
    pub fn created(record: &JobRecord) -> Self {
        Self::new(
            record,
            JobEventDetail::Created {
                image_path: record.image_path.clone(),
                identity: record.resolved_identity.clone(),
                operation_id: record.operation_id,
            },
        )
    }

    pub fn started(record: &JobRecord) -> Result<Self, JobStoreError> {
        Ok(Self::new(
            record,
            JobEventDetail::Started {
                started_at_ns: started_at(record)?,
                pid: pid(record)?,
                cgroup_id: record.cgroup_id.clone(),
            },
        ))
    }

    pub fn ended(record: &JobRecord) -> Result<Self, JobStoreError> {
        Ok(Self::new(
            record,
            JobEventDetail::Ended {
                ended_at_ns: ended_at(record)?,
                duration_ns: duration(record)?,
                exit_code: record.exit_code,
                exit_signal: record.exit_signal,
                failure_cause: record.failure_cause.clone(),
            },
        ))
    }

    fn new(record: &JobRecord, detail: JobEventDetail) -> Self {
        Self {
            job_id: record.id,
            service: record.service.clone(),
            job_type: record.job_type,
            hook_index: record.hook_index,
            state: record.state,
            pid: record.pid,
            pidfd: record.pidfd,
            resolved_identity: record.resolved_identity.clone(),
            operation_id: record.operation_id,
            token_summary: record.token_summary.clone(),
            image_path: record.image_path.clone(),
            arguments: record.arguments.clone(),
            created_at_ns: record.created_at_ns,
            started_at_ns: record.started_at_ns,
            ended_at_ns: record.ended_at_ns,
            exit_code: record.exit_code,
            exit_signal: record.exit_signal,
            failure_cause: record.failure_cause.clone(),
            cgroup_id: record.cgroup_id.clone(),
            activation_generation: record.activation_generation,
            cgroup_generation: record.cgroup_generation,
            detail,
        }
    }
}

fn started_at(record: &JobRecord) -> Result<u64, JobStoreError> {
    record
        .started_at_ns
        .ok_or_else(|| invalid_record(record, "started job has start timestamp"))
}

fn pid(record: &JobRecord) -> Result<u32, JobStoreError> {
    record
        .pid
        .ok_or_else(|| invalid_record(record, "started job has pid"))
}

fn ended_at(record: &JobRecord) -> Result<u64, JobStoreError> {
    record
        .ended_at_ns
        .ok_or_else(|| invalid_record(record, "ended job has end timestamp"))
}

fn duration(record: &JobRecord) -> Result<u64, JobStoreError> {
    record
        .duration_ns()
        .ok_or_else(|| invalid_record(record, "ended job has duration"))
}

fn invalid_record(record: &JobRecord, reason: &'static str) -> JobStoreError {
    JobStoreError::InvalidEventRecord {
        id: record.id,
        state: record.state,
        reason,
    }
}
