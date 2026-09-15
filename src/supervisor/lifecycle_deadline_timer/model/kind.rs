use crate::ids::{JobId, OperationId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorLifecycleDeadlineKind {
    PreStartCheckTimeout {
        service: String,
        operation_id: OperationId,
        result_fd: i32,
        pidfd: i32,
    },
    PreStartHookTimeout {
        service: String,
        operation_id: OperationId,
        job_id: JobId,
    },
    PostStartHookTimeout {
        service: String,
        operation_id: OperationId,
        job_id: JobId,
    },
    ReadinessTimeout {
        service: String,
        operation_id: OperationId,
        job_id: JobId,
    },
    StopTimeout {
        service: String,
        operation_id: OperationId,
    },
    ReloadDetection {
        service: String,
        operation_id: OperationId,
    },
    ReloadCommandTimeout {
        service: String,
        operation_id: OperationId,
        job_id: JobId,
    },
    RestartBackoff {
        service: String,
    },
    HealthCheckInterval {
        service: String,
        generation: u64,
    },
    HealthCheckTimeout {
        service: String,
        generation: u64,
        job_id: JobId,
    },
    WatchdogTimeout {
        service: String,
        generation: u64,
    },
    CgroupCleanup {
        service: String,
        cgroup_id: String,
    },
    BootSuccess,
    BootSettle,
    SubmittedJob {
        job_id: JobId,
        kind: crate::submitted::SubmittedJobDeadlineKind,
    },
}

impl SupervisorLifecycleDeadlineKind {
    pub(in crate::supervisor::lifecycle_deadline_timer) fn rank(&self) -> u8 {
        match self {
            Self::PreStartCheckTimeout { .. } => 0,
            Self::PreStartHookTimeout { .. } => 1,
            Self::PostStartHookTimeout { .. } => 2,
            Self::ReadinessTimeout { .. } => 3,
            Self::StopTimeout { .. } => 4,
            Self::ReloadDetection { .. } => 5,
            Self::ReloadCommandTimeout { .. } => 6,
            Self::RestartBackoff { .. } => 7,
            Self::HealthCheckTimeout { .. } => 8,
            Self::WatchdogTimeout { .. } => 9,
            Self::HealthCheckInterval { .. } => 10,
            Self::CgroupCleanup { .. } => 11,
            Self::BootSuccess => 12,
            Self::BootSettle => 13,
            Self::SubmittedJob { .. } => 14,
        }
    }

    pub(in crate::supervisor::lifecycle_deadline_timer) fn service(&self) -> &str {
        match self {
            Self::PreStartCheckTimeout { service, .. }
            | Self::PreStartHookTimeout { service, .. }
            | Self::PostStartHookTimeout { service, .. }
            | Self::ReadinessTimeout { service, .. }
            | Self::StopTimeout { service, .. }
            | Self::ReloadDetection { service, .. }
            | Self::ReloadCommandTimeout { service, .. }
            | Self::RestartBackoff { service }
            | Self::HealthCheckInterval { service, .. }
            | Self::HealthCheckTimeout { service, .. }
            | Self::WatchdogTimeout { service, .. }
            | Self::CgroupCleanup { service, .. } => service,
            Self::BootSuccess | Self::BootSettle | Self::SubmittedJob { .. } => "",
        }
    }

    pub(super) fn operation_id(&self) -> Option<OperationId> {
        match self {
            Self::PreStartCheckTimeout { operation_id, .. }
            | Self::PreStartHookTimeout { operation_id, .. }
            | Self::PostStartHookTimeout { operation_id, .. }
            | Self::ReadinessTimeout { operation_id, .. }
            | Self::StopTimeout { operation_id, .. }
            | Self::ReloadDetection { operation_id, .. }
            | Self::ReloadCommandTimeout { operation_id, .. } => Some(*operation_id),
            Self::RestartBackoff { .. } | Self::BootSuccess | Self::BootSettle => None,
            Self::HealthCheckInterval { .. }
            | Self::HealthCheckTimeout { .. }
            | Self::WatchdogTimeout { .. }
            | Self::CgroupCleanup { .. }
            | Self::SubmittedJob { .. } => None,
        }
    }

    pub(in crate::supervisor::lifecycle_deadline_timer) fn job_id(&self) -> Option<JobId> {
        match self {
            Self::PreStartHookTimeout { job_id, .. }
            | Self::PostStartHookTimeout { job_id, .. }
            | Self::ReadinessTimeout { job_id, .. }
            | Self::ReloadCommandTimeout { job_id, .. } => Some(*job_id),
            Self::HealthCheckTimeout { job_id, .. } | Self::SubmittedJob { job_id, .. } => {
                Some(*job_id)
            }
            Self::PreStartCheckTimeout { .. }
            | Self::StopTimeout { .. }
            | Self::ReloadDetection { .. }
            | Self::RestartBackoff { .. }
            | Self::HealthCheckInterval { .. }
            | Self::WatchdogTimeout { .. }
            | Self::CgroupCleanup { .. }
            | Self::BootSuccess
            | Self::BootSettle => None,
        }
    }
}
