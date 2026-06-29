use std::collections::BTreeMap;

use crate::boot::phase2::BlockedReason;

use super::model::BlockedServiceDraft;

pub(in crate::boot::phase2::graph) fn block_service(
    blocked: &mut BTreeMap<String, BlockedServiceDraft>,
    service: &str,
    reason: BlockedReason,
) {
    match blocked.get_mut(service) {
        Some(existing)
            if failed_cause_precedence(&reason) > failed_cause_precedence(&existing.reason) =>
        {
            existing.reason = reason;
        }
        Some(_) => {}
        None => {
            blocked.insert(
                service.to_string(),
                BlockedServiceDraft {
                    service: service.to_string(),
                    reason,
                },
            );
        }
    }
}

fn failed_cause_precedence(reason: &BlockedReason) -> u8 {
    match reason {
        BlockedReason::CycleDetected { .. } => 3,
        BlockedReason::ConflictingBootService { .. } | BlockedReason::ValidationError { .. } => 2,
        BlockedReason::HardDependencyUnavailable { .. }
        | BlockedReason::HardDependencyBlocked { .. } => 1,
    }
}
