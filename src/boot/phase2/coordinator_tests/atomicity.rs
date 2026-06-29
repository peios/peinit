use crate::boot::phase2::{Phase2BootPlanError, Phase2BootRunError, run_phase2_boot};
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::operation::store::{OperationRequest, OperationStore, OperationStoreError};
use crate::operation::{OperationSource, OperationType};

use super::{
    FixedClock, OBSERVED_AT_NS, StaticRegistry, allocated_operation_id, service, settings,
};

#[test]
fn graph_validation_error_does_not_advance_ids_or_dispatch_operations() {
    let app = service("app", "/sbin/app");
    let duplicate = service("app", "/sbin/app2");
    let mut registry = StaticRegistry::services(vec![app, duplicate]);
    let mut clock = FixedClock::at(OBSERVED_AT_NS);
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let mut operations = OperationStore::new();

    let err = run_phase2_boot(
        settings(),
        &mut registry,
        &mut clock,
        &mut operation_ids,
        &mut job_ids,
        &mut operations,
    )
    .expect_err("duplicate service");

    assert_eq!(
        err,
        Phase2BootRunError::Plan(Phase2BootPlanError::DuplicateService {
            service: "app".to_string(),
        }),
    );
    assert_eq!(operation_ids.next_sequence(), 0);
    assert_eq!(job_ids.next_sequence(), 0);
    assert!(operations.active_for_service("app").is_empty());
}

#[test]
fn dispatch_error_does_not_commit_allocators_or_operation_store() {
    let duplicate_id = allocated_operation_id(0);
    let mut operations = OperationStore::new();
    operations
        .request_operation(OperationRequest {
            id: duplicate_id,
            operation_type: OperationType::Start,
            service: "preexisting".to_string(),
            source: OperationSource::Admin,
            caller: None,
            created_at_ns: OBSERVED_AT_NS,
        })
        .expect("preexisting operation");
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")]);
    let mut clock = FixedClock::at(OBSERVED_AT_NS);
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();

    let err = run_phase2_boot(
        settings(),
        &mut registry,
        &mut clock,
        &mut operation_ids,
        &mut job_ids,
        &mut operations,
    )
    .expect_err("duplicate operation id");

    assert_eq!(
        err,
        Phase2BootRunError::Dispatch(OperationStoreError::DuplicateOperationId {
            id: duplicate_id,
        }),
    );
    assert_eq!(operation_ids.next_sequence(), 0);
    assert_eq!(job_ids.next_sequence(), 0);
    assert_eq!(
        operations.active_for_service("preexisting"),
        vec![duplicate_id]
    );
    assert!(operations.active_for_service("app").is_empty());
}
