use crate::execution::graph::ReadyGraphOperation;
use crate::ids::OperationId;
use crate::security::TokenSummary;
use crate::service::{ServiceActivationSnapshot, ServiceCheck};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPreStartCheck {
    pub operation_id: OperationId,
    pub service: String,
    pub activation: ServiceActivationSnapshot,
    pub checks: Vec<ServiceCheck>,
    pub started_at_ns: u64,
    pub timeout_secs: u64,
    pub operation_deadline_ns: u64,
    pub start: PendingPreStartCheckStart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingPreStartCheckStart {
    Graph {
        ready: ReadyGraphOperation,
        resolved_identity: String,
        token_summary: TokenSummary,
    },
    GraphPreDependency {
        ready: ReadyGraphOperation,
        resolved_identity: String,
        token_summary: TokenSummary,
    },
    Restart {
        resolved_identity: String,
        token_summary: TokenSummary,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPreStartCheckRegistration {
    pub checks: Vec<ServiceCheck>,
    pub helper_cgroup_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecheckedGraphStart {
    pub ready: ReadyGraphOperation,
    pub activation: ServiceActivationSnapshot,
    pub resolved_identity: String,
    pub token_summary: TokenSummary,
    pub checked_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningPreStartCheckHelper {
    pub helper: crate::boundary::LaunchedFilesystemCheckHelper,
    pub pending: PendingPreStartCheck,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreStartCheckDeadline {
    pub operation_id: OperationId,
    pub service: String,
    pub helper_cgroup_id: String,
    pub result_fd: i32,
    pub pidfd: i32,
    pub due_at_ns: u64,
}

impl PendingPreStartCheck {
    pub fn new(
        operation_id: OperationId,
        service: String,
        activation: ServiceActivationSnapshot,
        checks: Vec<ServiceCheck>,
        started_at_ns: u64,
        operation_deadline_ns: u64,
        start: PendingPreStartCheckStart,
    ) -> Self {
        let timeout_secs = activation.definition.pre_start_check_timeout_secs;
        Self {
            operation_id,
            service,
            activation,
            checks,
            started_at_ns,
            timeout_secs,
            operation_deadline_ns,
            start,
        }
    }

    pub fn service_cgroup_id(&self) -> String {
        crate::job::service_cgroup_root_path(&self.service, self.activation.cgroup_generation)
    }

    pub fn helper_cgroup_id(&self) -> String {
        format!("{}/checks", self.service_cgroup_id())
    }
}
