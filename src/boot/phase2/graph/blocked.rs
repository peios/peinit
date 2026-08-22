use std::collections::BTreeMap;

use crate::boot::phase2::BlockedReason;

use super::model::BlockedServiceDraft;

/// Record a validation finding against a service, retaining every finding.
///
/// PSD-007 §6.2 splits two things that are easy to conflate. The service
/// runtime state records ONE primary Failed cause, chosen by precedence
/// (CycleDetected > ValidationError > DependencyFailure). The diagnostics MUST
/// still carry all of them: "This precedence affects only the primary cause
/// stored on the service state. It MUST NOT suppress logging of the
/// lower-precedence findings."
///
/// So `reason` stays the single primary — it is what feeds
/// `BlockedReason::transition_cause()` — and everything else accumulates in
/// `additional_reasons`, which the operation failure message enumerates. A
/// service in a dependency cycle AND missing a `Requires` target used to
/// report only the cycle; breaking the cycle and rebooting was the only way to
/// discover the second fault, which was known at the same instant.
pub(in crate::boot::phase2::graph) fn block_service(
    blocked: &mut BTreeMap<String, BlockedServiceDraft>,
    service: &str,
    reason: BlockedReason,
) {
    let Some(existing) = blocked.get_mut(service) else {
        blocked.insert(
            service.to_string(),
            BlockedServiceDraft {
                service: service.to_string(),
                reason,
                additional_reasons: Vec::new(),
            },
        );
        return;
    };

    // A finding already recorded for this service adds nothing to the
    // diagnosis and would just repeat itself in the message.
    if existing.reason == reason || existing.additional_reasons.contains(&reason) {
        return;
    }

    if failed_cause_precedence(&reason) > failed_cause_precedence(&existing.reason) {
        // The new finding outranks the stored primary. The old primary is
        // demoted rather than dropped: it is still a real fault with the
        // service, and losing it here is the bug this function exists to fix.
        let demoted = std::mem::replace(&mut existing.reason, reason);
        existing.additional_reasons.push(demoted);
    } else {
        existing.additional_reasons.push(reason);
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
