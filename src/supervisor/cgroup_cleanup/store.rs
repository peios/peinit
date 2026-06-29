use std::collections::BTreeMap;

use super::model::{CgroupCleanupDeadline, CgroupCleanupKind};

const NANOS_PER_SEC: u64 = 1_000_000_000;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::supervisor) struct CgroupCleanupStore {
    deadlines: BTreeMap<String, CgroupCleanupDeadline>,
}

impl CgroupCleanupStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, deadline: CgroupCleanupDeadline) {
        match self.deadlines.get_mut(&deadline.cgroup_id) {
            Some(existing) if existing.due_at_ns <= deadline.due_at_ns => {}
            Some(existing) => *existing = deadline,
            None => {
                self.deadlines.insert(deadline.cgroup_id.clone(), deadline);
            }
        }
    }

    pub fn next_deadline(&self) -> Option<CgroupCleanupDeadline> {
        self.deadlines
            .values()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
            .cloned()
    }

    pub fn due_deadlines(&self, now_ns: u64) -> Vec<CgroupCleanupDeadline> {
        self.deadlines
            .values()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .cloned()
            .collect()
    }

    pub fn remove(&mut self, cgroup_id: &str) -> Option<CgroupCleanupDeadline> {
        self.deadlines.remove(cgroup_id)
    }
}

fn cgroup_cleanup_due_at_ns(now_ns: u64, post_kill_timeout_secs: u64) -> u64 {
    now_ns.saturating_add(post_kill_timeout_secs.saturating_mul(NANOS_PER_SEC))
}

pub(in crate::supervisor) fn record_cgroup_cleanup(
    store: &mut CgroupCleanupStore,
    service: impl Into<String>,
    cgroup_id: impl Into<String>,
    kind: CgroupCleanupKind,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) {
    store.record(CgroupCleanupDeadline {
        service: service.into(),
        cgroup_id: cgroup_id.into(),
        kind,
        due_at_ns: cgroup_cleanup_due_at_ns(now_ns, post_kill_timeout_secs),
    });
}
