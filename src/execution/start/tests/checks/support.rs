use crate::boundary::{
    FilesystemCheckReport, FilesystemCheckResult, LaunchedFilesystemCheckHelper,
};
use crate::control::lifecycle::{OnDemandStartDispatch, OnDemandStartPlan, PlannedStart};
use crate::execution::start::{RestartStartExecutionRequest, StartExecutionStore};
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::{OperationRequest, OperationStore};
use crate::operation::{OperationSource, OperationType};
use crate::service::runtime::TransitionCause;
use crate::service::{ServiceCheck, ServiceCheckKind};

use super::super::{OBSERVED_AT_NS, STARTED_AT_NS, operation_id, token_summary};

pub(super) fn registry_check(argument: &str) -> ServiceCheck {
    ServiceCheck {
        kind: ServiceCheckKind::Registry,
        argument: argument.to_string(),
    }
}

pub(super) fn record_running_helper(
    start_store: &mut StartExecutionStore,
    service: &str,
    operation_id: crate::ids::OperationId,
    checks: Vec<ServiceCheck>,
) {
    start_store
        .pop_pending_pre_start_check_launch()
        .expect("pending helper launch");
    start_store
        .record_running_pre_start_check_helper(
            LaunchedFilesystemCheckHelper {
                service: service.to_string(),
                operation_id,
                checks,
                pid: 8001,
                pidfd: 80,
                result_fd: 81,
                cgroup_id: "/sys/fs/cgroup/peinit/app/checks".to_string(),
            },
            STARTED_AT_NS,
        )
        .expect("running helper");
}

pub(super) fn report(
    service: &str,
    operation_id: crate::ids::OperationId,
    results: Vec<(ServiceCheck, bool)>,
) -> FilesystemCheckReport {
    FilesystemCheckReport {
        service: service.to_string(),
        operation_id,
        results: results
            .into_iter()
            .map(|(check, satisfied)| FilesystemCheckResult { check, satisfied })
            .collect(),
    }
}

pub(super) fn running_restart_operation(service: &str) -> OperationStore {
    let operation_id = operation_id(0);
    let mut operations = OperationStore::new();
    operations
        .request_operation(OperationRequest {
            id: operation_id,
            operation_type: OperationType::Restart,
            service: service.to_string(),
            source: OperationSource::Admin,
            caller: Some(token_summary()),
            created_at_ns: OBSERVED_AT_NS,
        })
        .expect("request restart");
    operations
        .start_operation(operation_id, STARTED_AT_NS)
        .expect("start restart");
    operations
}

pub(super) fn restart_request(
    service: &str,
    operation_id: crate::ids::OperationId,
) -> RestartStartExecutionRequest {
    RestartStartExecutionRequest {
        service: service.to_string(),
        operation_id,
        resolved_identity: "SYSTEM".to_string(),
        token_summary: token_summary(),
        started_at_ns: STARTED_AT_NS,
    }
}

pub(super) fn on_demand_dispatch(
    service: &str,
    requested_operation: crate::operation::store::OperationRequestOutcome,
) -> OnDemandStartDispatch {
    OnDemandStartDispatch {
        plan: OnDemandStartPlan {
            requested: service.to_string(),
            requested_operation_source: crate::operation::OperationSource::Admin,
            requested_transition_cause: TransitionCause::ExplicitStart,
            starts: vec![PlannedStart {
                service: service.to_string(),
                operation_source: crate::operation::OperationSource::Admin,
                transition_cause: TransitionCause::ExplicitStart,
            }],
            blocked: Vec::new(),
        },
        requested_operation: crate::operation::store::OperationRequestOutcome {
            decision: OperationConflictDecision::CreateNew,
            ..requested_operation
        },
        dependency_operations: Vec::new(),
        events: Vec::new(),
    }
}
