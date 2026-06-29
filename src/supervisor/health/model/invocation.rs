use crate::ids::JobId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::supervisor) enum HealthCheckInvocation {
    Pending {
        job_id: JobId,
        health_cgroup_id: String,
    },
    Running(RunningHealthCheckInvocation),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::supervisor) struct RunningHealthCheckInvocation {
    pub job_id: JobId,
    pub health_cgroup_id: String,
    pub timeout_due_at_ns: u64,
}

impl HealthCheckInvocation {
    pub fn job_id(&self) -> JobId {
        match self {
            Self::Pending { job_id, .. } => *job_id,
            Self::Running(running) => running.job_id,
        }
    }
}
