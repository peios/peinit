use crate::ids::JobId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthCheckIntervalDeadline {
    pub service: String,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub due_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthCheckTimeoutDeadline {
    pub service: String,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub job_id: JobId,
    pub health_cgroup_id: String,
    pub due_at_ns: u64,
}
