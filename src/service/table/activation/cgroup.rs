use crate::service::runtime::{LeakedCgroup, LeakedCgroupKind};

use super::super::ServiceTable;
use super::super::model::ServiceTableError;

impl ServiceTable {
    pub fn record_leaked_cgroup(
        &mut self,
        service: &str,
        path: String,
        kind: LeakedCgroupKind,
        detected_at_ns: u64,
    ) -> Result<(), ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        if entry
            .runtime
            .leaked_cgroups
            .iter()
            .any(|leak| leak.path == path && leak.kind == kind)
        {
            return Ok(());
        }
        entry.runtime.leaked_cgroups.push(LeakedCgroup {
            path,
            kind,
            detected_at_ns,
        });
        entry.runtime.cgroup_generation = entry.runtime.cgroup_generation.saturating_add(1);
        Ok(())
    }
}
