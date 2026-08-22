use crate::boot::phase2::{BlockedReason, BlockedService, DependencyKind, Phase2BootPlan};
use crate::ids::OperationId;
use crate::operation::store::{
    OperationEvent, OperationRequest, OperationStore, OperationStoreError,
};
use crate::operation::{OperationSource, OperationType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase2BootDispatch {
    pub blocked_operation_ids: Vec<OperationId>,
    pub start_operation_ids: Vec<OperationId>,
    pub events: Vec<OperationEvent>,
}

impl OperationStore {
    pub fn dispatch_phase2_boot_plan(
        &mut self,
        plan: &Phase2BootPlan,
    ) -> Result<Phase2BootDispatch, OperationStoreError> {
        let mut events = Vec::new();
        let mut blocked_operation_ids = Vec::new();
        let mut start_operation_ids = Vec::new();

        for blocked in &plan.blocked {
            let request = OperationRequest {
                id: blocked.operation_id,
                operation_type: OperationType::Start,
                service: blocked.service.clone(),
                source: OperationSource::Boot,
                caller: None,
                created_at_ns: plan.observed_at_ns,
            };
            events.extend(self.request_operation(request)?.events);
            events.push(self.fail_operation(
                blocked.operation_id,
                plan.observed_at_ns,
                blocked_failure_message(blocked),
            )?);
            blocked_operation_ids.push(blocked.operation_id);
        }

        for start in &plan.starts {
            let request = OperationRequest {
                id: start.operation_id,
                operation_type: OperationType::Start,
                service: start.service.clone(),
                source: OperationSource::Boot,
                caller: None,
                created_at_ns: plan.observed_at_ns,
            };
            events.extend(self.request_operation(request)?.events);
            start_operation_ids.push(start.operation_id);
        }

        Ok(Phase2BootDispatch {
            blocked_operation_ids,
            start_operation_ids,
            events,
        })
    }
}

/// The operation's failure message: the primary cause, then every other
/// finding for the same service.
///
/// PSD-007 §6.2 requires all findings be logged even though only one becomes
/// the service's recorded Failed cause, and §5.2 is emphatic about why —
/// "cryptic failure messages are a specification violation". An administrator
/// whose service is both in a cycle and missing a `Requires` target should
/// learn both facts from one boot, not one per reboot.
///
/// The primary stays first and unqualified so the leading text is unchanged
/// from the single-finding case, which is the common one.
fn blocked_failure_message(blocked: &BlockedService) -> String {
    let primary = blocked_failure_reason(&blocked.reason);
    if blocked.additional_reasons.is_empty() {
        return primary;
    }
    let others = blocked
        .additional_reasons
        .iter()
        .map(blocked_failure_reason)
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "{primary} (also: {others}) [{} findings]",
        blocked.additional_reasons.len() + 1
    )
}

fn blocked_failure_reason(reason: &BlockedReason) -> String {
    match reason {
        BlockedReason::HardDependencyUnavailable { target, kind } => {
            format!(
                "DependencyFailure: {} dependency {target} is unavailable",
                dependency_kind_name(*kind)
            )
        }
        BlockedReason::HardDependencyBlocked { target, kind } => {
            format!(
                "DependencyFailure: {} dependency {target} is blocked",
                dependency_kind_name(*kind)
            )
        }
        BlockedReason::CycleDetected { services } => {
            format!("CycleDetected: dependency cycle {}", services.join(" -> "))
        }
        BlockedReason::ConflictingBootService { target } => {
            format!("ValidationError: boot conflict with {target}")
        }
        BlockedReason::ValidationError { message } => {
            format!("ValidationError: {message}")
        }
    }
}

fn dependency_kind_name(kind: DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Requires => "Requires",
        DependencyKind::Wants => "Wants",
        DependencyKind::BindsTo => "BindsTo",
    }
}
