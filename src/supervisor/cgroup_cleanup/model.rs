use crate::ids::OperationId;
use crate::service::runtime::LeakedCgroupKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::supervisor) struct CgroupCleanupReport {
    pub removed: Vec<String>,
    pub missing: Vec<String>,
    pub busy: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::supervisor) struct CgroupCleanupDeadline {
    pub service: String,
    pub cgroup_id: String,
    pub kind: CgroupCleanupKind,
    pub due_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::supervisor) enum CgroupCleanupKind {
    Hooks,
    Health,
    Helper,
    ServiceTree,
    StopMain {
        operation_id: OperationId,
        root_cgroup_id: String,
    },
}

/// A sub-cgroup that could not be reclaimed, reported out of the turn that
/// detected it.
///
/// A leak is also recorded on the service's runtime state, where a `status`
/// query and a `start` acknowledgement find it -- but both of those are pull,
/// and nobody is necessarily looking at the service that leaked. What a leak
/// means is that something underneath the service has stopped answering the
/// kernel, so it has to reach an operator who was not asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorLeakedCgroupDispatch {
    pub service: String,
    pub path: String,
    pub kind: LeakedCgroupKind,
    pub detected_at_ns: u64,
}
