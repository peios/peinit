use std::path::Path;

use crate::boundary::{BoundaryError, CgroupRemoveOutcome, ProcessController};

use super::model::CgroupCleanupReport;

pub(in crate::supervisor) fn cleanup_service_cgroup_tree<P>(
    controller: &mut P,
    root_cgroup_id: &str,
) -> Result<CgroupCleanupReport, BoundaryError>
where
    P: ProcessController + ?Sized,
{
    let mut report = CgroupCleanupReport {
        removed: Vec::new(),
        missing: Vec::new(),
        busy: Vec::new(),
    };
    for cgroup_id in service_cgroup_cleanup_order(root_cgroup_id) {
        match controller.remove_cgroup(&cgroup_id)? {
            CgroupRemoveOutcome::Removed => report.removed.push(cgroup_id),
            CgroupRemoveOutcome::Missing => report.missing.push(cgroup_id),
            CgroupRemoveOutcome::Busy => report.busy.push(cgroup_id),
        }
    }
    Ok(report)
}

fn service_cgroup_cleanup_order(root_cgroup_id: &str) -> Vec<String> {
    let root = Path::new(root_cgroup_id);
    ["main", "hooks", "health"]
        .into_iter()
        .map(|child| root.join(child).to_string_lossy().into_owned())
        .chain(std::iter::once(root_cgroup_id.to_string()))
        .collect()
}

pub(in crate::supervisor) fn parent_cgroup_path(cgroup_id: &str) -> Option<String> {
    Path::new(cgroup_id)
        .parent()
        .map(|path| path.to_string_lossy().into_owned())
}
