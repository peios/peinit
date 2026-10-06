use crate::boundary::{BoundaryError, KmesEvent};
use crate::ids::JobId;
use crate::service::runtime::LeakedCgroupKind;
use crate::supervisor::SupervisorLeakedCgroupDispatch;

use crate::kmes::payload::Payload;
use crate::kmes::types::CGROUP_LEAKED;

/// `peinit.cgroup.leaked`: one of a service's sub-cgroups could not be
/// reclaimed.
pub fn encode_leaked_cgroup_event(
    event: &SupervisorLeakedCgroupDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("object.service.name", event.service.as_str());
    payload.set("object.cgroup.path", event.path.as_str());
    payload.set("object.cgroup.type", leaked_cgroup_kind(event.kind));
    payload.finish(CGROUP_LEAKED)
}

/// `peinit.cgroup.leaked` for a submitted job's cgroup, which belongs to the
/// job and to no service: the job is named, and the cgroup is the whole of
/// its containment.
pub fn encode_leaked_job_cgroup_event(
    job_id: JobId,
    path: &str,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("object.job.guid", job_id);
    payload.set("object.cgroup.path", path);
    payload.set(
        "object.cgroup.type",
        leaked_cgroup_kind(LeakedCgroupKind::ServiceTree),
    );
    payload.finish(CGROUP_LEAKED)
}

/// The words a `status` query's `warnings` array uses, written as an event
/// value is, in kebab-case.
fn leaked_cgroup_kind(kind: LeakedCgroupKind) -> &'static str {
    match kind {
        LeakedCgroupKind::ServiceTree => "service-tree",
        LeakedCgroupKind::Health => "health",
        LeakedCgroupKind::Hooks => "hooks",
        LeakedCgroupKind::Helper => "helper",
    }
}
