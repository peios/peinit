mod boot;
mod checks;
mod on_demand;

use crate::control::lifecycle::{OnDemandStartDispatch, OnDemandStartPlan, PlannedStart};
use crate::ids::OperationIdAllocator;
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::{OperationRequest, OperationRequestOutcome, OperationStore};
use crate::operation::{OperationSource, OperationType};
use crate::security::TokenSummary;
use crate::service::ServiceDefinition;
use crate::service::runtime::TransitionCause;

use super::StartExecutionRequest;

const OBSERVED_AT_NS: u64 = 1_000_000_000;
const STARTED_AT_NS: u64 = 1_000_001_000;

fn service(name: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"))
}

fn start_request(ready: crate::execution::graph::ReadyGraphOperation) -> StartExecutionRequest {
    StartExecutionRequest {
        ready,
        resolved_identity: "SYSTEM".to_string(),
        token_summary: token_summary(),
        started_at_ns: STARTED_AT_NS,
    }
}

fn token_summary() -> TokenSummary {
    TokenSummary::requested_identity("SYSTEM")
}

fn operation_id(sequence: usize) -> crate::ids::OperationId {
    OperationIdAllocator::new()
        .allocate_batch(sequence + 1, OBSERVED_AT_NS)
        .expect("operation ids")[sequence]
}

fn request_admin_start(
    operations: &mut OperationStore,
    operation_id: crate::ids::OperationId,
    service: &str,
) -> OperationRequestOutcome {
    operations
        .request_operation(OperationRequest {
            id: operation_id,
            operation_type: OperationType::Start,
            service: service.to_string(),
            source: OperationSource::Admin,
            caller: Some(token_summary()),
            created_at_ns: OBSERVED_AT_NS,
        })
        .expect("request start")
}

fn on_demand_dispatch(
    service: &str,
    requested_operation: OperationRequestOutcome,
) -> OnDemandStartDispatch {
    OnDemandStartDispatch {
        plan: OnDemandStartPlan {
            requested: service.to_string(),
            requested_operation_source: OperationSource::Admin,
            requested_transition_cause: TransitionCause::ExplicitStart,
            starts: vec![PlannedStart {
                service: service.to_string(),
                operation_source: OperationSource::Admin,
                transition_cause: TransitionCause::ExplicitStart,
            }],
            blocked: Vec::new(),
        },
        requested_operation: OperationRequestOutcome {
            decision: OperationConflictDecision::CreateNew,
            ..requested_operation
        },
        dependency_operations: Vec::new(),
        events: Vec::new(),
    }
}
