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
    ) -> Result<bool, ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        if entry
            .runtime
            .leaked_cgroups
            .iter()
            .any(|leak| leak.path == path && leak.kind == kind)
        {
            return Ok(false);
        }
        // Only a leak in the tree the service is *currently* using needs a new
        // generation. A leak recorded against an older tree is already behind
        // the current one, and bumping for it would move the service on for no
        // reason.
        //
        // This is what makes the increment per-tree rather than per-record. One
        // failed start records two cleanup deadlines -- `hooks` and
        // `ServiceTree` -- and a tree cleanup can report `main`, `hooks`,
        // `health` and the root separately; all of them live under the same
        // root, so the first advances the generation and the rest find
        // themselves already behind it. Before this, N jumped by however many
        // paths happened to be unreclaimable (PEI-353).
        let advances = leak_affects_tree(
            &path,
            &crate::job::service_cgroup_root_path(service, entry.runtime.cgroup_generation),
        );
        entry.runtime.leaked_cgroups.push(LeakedCgroup {
            path,
            kind,
            detected_at_ns,
        });
        if advances {
            entry.runtime.cgroup_generation = entry.runtime.cgroup_generation.saturating_add(1);
        }
        Ok(true)
    }
}

/// Whether a leaked path is the given tree's root, or something inside it.
fn leak_affects_tree(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}
