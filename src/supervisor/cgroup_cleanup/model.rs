use crate::ids::OperationId;

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
