use crate::boot::phase2::Phase2BootPlan;
use crate::ids::OperationId;
use crate::security::TokenSummary;

use super::model::{OperationRecord, OperationSource, OperationState, OperationType};

impl OperationRecord {
    pub fn new(
        id: OperationId,
        operation_type: OperationType,
        service: impl Into<String>,
        source: OperationSource,
        caller: Option<TokenSummary>,
        created_at_ns: u64,
    ) -> Self {
        Self {
            id,
            operation_type,
            service: service.into(),
            state: OperationState::Pending,
            created_at_ns,
            started_at_ns: None,
            completed_at_ns: None,
            source,
            caller,
            result: None,
            merged_into: None,
            service_security: None,
        }
    }
}

pub fn boot_start_operations_from_phase2_plan(plan: &Phase2BootPlan) -> Vec<OperationRecord> {
    plan.starts
        .iter()
        .map(|start| {
            OperationRecord::new(
                start.operation_id,
                OperationType::Start,
                start.service.clone(),
                OperationSource::Boot,
                None,
                plan.observed_at_ns,
            )
        })
        .collect()
}
