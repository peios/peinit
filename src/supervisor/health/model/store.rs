use std::collections::BTreeMap;

use crate::ids::JobId;

use super::deadline::{HealthCheckIntervalDeadline, HealthCheckTimeoutDeadline};
use super::invocation::{HealthCheckInvocation, RunningHealthCheckInvocation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::supervisor) struct HealthCheckStore {
    records: BTreeMap<String, HealthCheckRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HealthCheckRecord {
    service: String,
    activation_generation: u64,
    cgroup_generation: u64,
    next_interval_due_at_ns: u64,
    invocation: Option<HealthCheckInvocation>,
}

impl HealthCheckStore {
    pub fn new() -> Self {
        Self {
            records: BTreeMap::new(),
        }
    }

    pub fn schedule_interval(
        &mut self,
        service: &str,
        activation_generation: u64,
        cgroup_generation: u64,
        due_at_ns: u64,
    ) {
        let invocation = self
            .records
            .get(service)
            .filter(|record| {
                record.activation_generation == activation_generation
                    && record.cgroup_generation == cgroup_generation
            })
            .and_then(|record| record.invocation.clone());
        self.records.insert(
            service.to_string(),
            HealthCheckRecord {
                service: service.to_string(),
                activation_generation,
                cgroup_generation,
                next_interval_due_at_ns: due_at_ns,
                invocation,
            },
        );
    }

    pub fn cancel_service(&mut self, service: &str) -> Option<HealthCheckInvocation> {
        self.records
            .remove(service)
            .and_then(|record| record.invocation)
    }

    pub fn has_invocation(&self, service: &str, activation_generation: u64) -> bool {
        self.records.get(service).is_some_and(|record| {
            record.activation_generation == activation_generation && record.invocation.is_some()
        })
    }

    pub fn record_pending_invocation(
        &mut self,
        service: &str,
        activation_generation: u64,
        job_id: JobId,
        health_cgroup_id: String,
    ) -> bool {
        let Some(record) = self.records.get_mut(service) else {
            return false;
        };
        if record.activation_generation != activation_generation || record.invocation.is_some() {
            return false;
        }
        record.invocation = Some(HealthCheckInvocation::Pending {
            job_id,
            health_cgroup_id,
        });
        true
    }

    pub fn mark_invocation_running(
        &mut self,
        job_id: JobId,
        timeout_due_at_ns: u64,
    ) -> Option<RunningHealthCheckInvocation> {
        let record = self.record_by_job_mut(job_id)?;
        let Some(HealthCheckInvocation::Pending {
            job_id,
            health_cgroup_id,
        }) = record.invocation.take()
        else {
            return None;
        };
        let running = RunningHealthCheckInvocation {
            job_id,
            health_cgroup_id,
            timeout_due_at_ns,
        };
        record.invocation = Some(HealthCheckInvocation::Running(running.clone()));
        Some(running)
    }

    pub fn remove_invocation(&mut self, job_id: JobId) -> Option<HealthCheckInvocation> {
        let record = self.record_by_job_mut(job_id)?;
        record.invocation.take()
    }

    pub fn next_interval_deadline(&self) -> Option<HealthCheckIntervalDeadline> {
        self.interval_deadlines()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
    }

    pub fn due_interval_deadlines(&self, now_ns: u64) -> Vec<HealthCheckIntervalDeadline> {
        self.interval_deadlines()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .collect()
    }

    pub fn next_timeout_deadline(&self) -> Option<HealthCheckTimeoutDeadline> {
        self.timeout_deadlines()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
    }

    pub fn due_timeout_deadlines(&self, now_ns: u64) -> Vec<HealthCheckTimeoutDeadline> {
        self.timeout_deadlines()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .collect()
    }

    fn interval_deadlines(&self) -> impl Iterator<Item = HealthCheckIntervalDeadline> + '_ {
        self.records
            .values()
            .map(|record| HealthCheckIntervalDeadline {
                service: record.service.clone(),
                activation_generation: record.activation_generation,
                cgroup_generation: record.cgroup_generation,
                due_at_ns: record.next_interval_due_at_ns,
            })
    }

    fn timeout_deadlines(&self) -> impl Iterator<Item = HealthCheckTimeoutDeadline> + '_ {
        self.records.values().filter_map(|record| {
            let Some(HealthCheckInvocation::Running(running)) = &record.invocation else {
                return None;
            };
            Some(HealthCheckTimeoutDeadline {
                service: record.service.clone(),
                activation_generation: record.activation_generation,
                cgroup_generation: record.cgroup_generation,
                job_id: running.job_id,
                health_cgroup_id: running.health_cgroup_id.clone(),
                due_at_ns: running.timeout_due_at_ns,
            })
        })
    }

    fn record_by_job_mut(&mut self, job_id: JobId) -> Option<&mut HealthCheckRecord> {
        self.records.values_mut().find(|record| {
            record
                .invocation
                .as_ref()
                .is_some_and(|invocation| invocation.job_id() == job_id)
        })
    }
}

impl Default for HealthCheckStore {
    fn default() -> Self {
        Self::new()
    }
}
