use crate::ids::JobId;
use crate::job::{ServiceCgroupKind, service_cgroup_root_path, service_job_cgroup_path};
use crate::operation::{OperationRecord, OperationType};
use crate::service::{Readiness, ServiceDefinition, ServiceType};

use super::store::{
    PostStartHookDeadline, PostStartHookSequence, PreStartHookDeadline, PreStartHookSequence,
    ReadinessDeadline,
};

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(super) fn pre_start_hook_deadline(
    sequence: &PreStartHookSequence,
    job_id: JobId,
) -> PreStartHookDeadline {
    PreStartHookDeadline {
        operation_id: sequence.operation_id,
        job_id,
        service: sequence.service.clone(),
        hooks_cgroup_id: service_job_cgroup_path(
            &sequence.service,
            sequence.cgroup_generation,
            ServiceCgroupKind::Hooks,
        ),
        service_cgroup_id: service_cgroup_root_path(&sequence.service, sequence.cgroup_generation),
        due_at_ns: sequence.deadline_ns,
    }
}

pub(super) fn post_start_hook_deadline(
    sequence: &PostStartHookSequence,
    job_id: JobId,
) -> PostStartHookDeadline {
    PostStartHookDeadline {
        operation_id: sequence.operation_id,
        job_id,
        service: sequence.service.clone(),
        hooks_cgroup_id: service_job_cgroup_path(
            &sequence.service,
            sequence.cgroup_generation,
            ServiceCgroupKind::Hooks,
        ),
        service_cgroup_id: service_cgroup_root_path(&sequence.service, sequence.cgroup_generation),
        due_at_ns: sequence.deadline_ns,
    }
}

pub(super) fn readiness_deadline(sequence: &PreStartHookSequence) -> Option<ReadinessDeadline> {
    requires_notify_readiness(&sequence.definition).then(|| ReadinessDeadline {
        operation_id: sequence.operation_id,
        job_id: sequence.main_job_id,
        service: sequence.service.clone(),
        service_cgroup_id: service_cgroup_root_path(&sequence.service, sequence.cgroup_generation),
        due_at_ns: sequence.deadline_ns,
    })
}

pub(super) fn requires_notify_readiness(definition: &ServiceDefinition) -> bool {
    definition.service_type == ServiceType::Simple && definition.readiness == Readiness::Notify
}

pub(super) fn start_deadline_ns(started_at_ns: u64, timeout_secs: u64) -> u64 {
    started_at_ns.saturating_add(timeout_secs.saturating_mul(NANOS_PER_SEC))
}

pub(super) fn start_operation_deadline_ns(
    operation: &OperationRecord,
    definition: &ServiceDefinition,
    started_at_ns: u64,
) -> u64 {
    // From the operation's lifetime origin, not its creation: the two differ
    // only for a start released from a hold (§7.5), whose clock starts at
    // the release (PEI-821).
    let start_leg_deadline_ns = start_deadline_ns(started_at_ns, definition.start_timeout_secs);
    match operation.operation_type {
        OperationType::Restart => start_leg_deadline_ns.min(
            operation.lifetime_from_ns.saturating_add(
                definition
                    .stop_timeout_secs
                    .saturating_add(definition.start_timeout_secs)
                    .saturating_mul(NANOS_PER_SEC),
            ),
        ),
        _ => operation
            .lifetime_from_ns
            .saturating_add(definition.start_timeout_secs.saturating_mul(NANOS_PER_SEC)),
    }
}
