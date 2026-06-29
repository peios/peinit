mod context;
mod schedule;
mod terminal;

use crate::boot::BootMode;
use crate::boot::phase2::{
    BlockedReason, BlockedService, DependencyKind, Phase2BootPlan, PreparedStart, StartCause,
};
use crate::control::lifecycle::{OnDemandStartDispatch, OnDemandStartPlan, PlannedStart};
use crate::ids::{JobId, JobIdAllocator, OperationId, OperationIdAllocator};
use crate::operation::OperationSource;
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::OperationRequestOutcome;
use crate::service::runtime::TransitionCause;
use crate::service::{ServiceDefinition, ServiceTable};

use super::{
    GraphContextBuildError, GraphContextKind, GraphExecutionError, GraphExecutionStore,
    GraphMemberStatus, GraphTerminalOutcome, ReadyGraphOperationAction,
};

const OBSERVED_AT_NS: u64 = 1_000_000_000;

fn service(name: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, &format!("/bin/{name}"))
}

fn boot_plan(starts: Vec<PreparedStart>) -> Phase2BootPlan {
    Phase2BootPlan {
        mode: BootMode::Full,
        observed_at_ns: OBSERVED_AT_NS,
        max_parallel_starts: 10,
        starts,
        blocked: Vec::new(),
    }
}

fn prepared_start(
    service: &str,
    operation_id: OperationId,
    job_id: JobId,
    cause: StartCause,
) -> PreparedStart {
    PreparedStart {
        service: service.to_string(),
        operation_id,
        job_id,
        cause,
        identity: "SYSTEM".to_string(),
    }
}

fn on_demand_dispatch(
    dependency: OperationRequestOutcome,
    requested_id: OperationId,
) -> OnDemandStartDispatch {
    OnDemandStartDispatch {
        plan: OnDemandStartPlan {
            requested: "app".to_string(),
            requested_operation_source: OperationSource::Admin,
            requested_transition_cause: TransitionCause::ExplicitStart,
            starts: vec![
                PlannedStart {
                    service: "db".to_string(),
                    operation_source: OperationSource::DependencyPropagation,
                    transition_cause: TransitionCause::DependencyStart,
                },
                PlannedStart {
                    service: "app".to_string(),
                    operation_source: OperationSource::Admin,
                    transition_cause: TransitionCause::ExplicitStart,
                },
            ],
            blocked: Vec::new(),
        },
        requested_operation: operation_outcome(requested_id, OperationConflictDecision::CreateNew),
        dependency_operations: vec![dependency],
        events: Vec::new(),
    }
}

fn operation_outcome(
    id: OperationId,
    decision: OperationConflictDecision,
) -> OperationRequestOutcome {
    OperationRequestOutcome {
        returned_operation_id: id,
        stored_operation_id: id,
        decision,
        events: Vec::new(),
    }
}

fn merged_operation_outcome(returned: OperationId, stored: OperationId) -> OperationRequestOutcome {
    OperationRequestOutcome {
        returned_operation_id: returned,
        stored_operation_id: stored,
        decision: OperationConflictDecision::MergeIntoExisting {
            existing_id: returned,
        },
        events: Vec::new(),
    }
}

fn operation_ids(count: usize) -> Vec<OperationId> {
    OperationIdAllocator::new()
        .allocate_batch(count, OBSERVED_AT_NS)
        .expect("operation ids")
}

fn job_ids(count: usize) -> Vec<JobId> {
    JobIdAllocator::new()
        .allocate_batch(count, OBSERVED_AT_NS)
        .expect("job ids")
}
