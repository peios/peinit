use crate::boot::BootMode;
use crate::boot::phase2::{
    Phase2BootRunError, Phase2BootSettings, Phase2RecoveryReason, run_phase2_boot,
};
use crate::boundary::BoundaryError;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::operation::store::OperationStore;

use super::{FixedClock, OBSERVED_AT_NS, StaticRegistry, service, settings};

#[test]
fn invalid_parallel_start_limit_requires_recovery_without_touching_boundaries() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")]);
    let mut clock = FixedClock::at(OBSERVED_AT_NS);
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let mut operations = OperationStore::new();

    let err = run_phase2_boot(
        Phase2BootSettings {
            mode: BootMode::Full,
            max_parallel_starts: 0,
            ..Phase2BootSettings::default()
        },
        &mut registry,
        &mut clock,
        &mut operation_ids,
        &mut job_ids,
        &mut operations,
    )
    .expect_err("invalid boot config");

    assert_eq!(
        err,
        Phase2BootRunError::RecoveryRequired(Phase2RecoveryReason::InvalidMaxParallelStarts),
    );
    assert_eq!(registry.reads, 0);
    assert_eq!(clock.reads, 0);
    assert_eq!(operation_ids.next_sequence(), 0);
    assert_eq!(job_ids.next_sequence(), 0);
}

#[test]
fn zero_registry_parallel_start_limit_requires_recovery() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .with_max_parallel_starts(Ok(Some(0)));
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
    .expect_err("invalid registry boot config");

    assert_eq!(
        err,
        Phase2BootRunError::RecoveryRequired(Phase2RecoveryReason::InvalidMaxParallelStarts),
    );
    assert_eq!(registry.reads, 0);
    assert_eq!(clock.reads, 0);
    assert_eq!(operation_ids.next_sequence(), 0);
    assert_eq!(job_ids.next_sequence(), 0);
}

#[test]
fn registry_read_failure_requires_recovery_without_reading_clock() {
    let registry_error = BoundaryError::Registry("timeout".to_string());
    let mut registry = StaticRegistry::error(registry_error.clone());
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
    .expect_err("registry recovery");

    assert_eq!(
        err,
        Phase2BootRunError::RecoveryRequired(Phase2RecoveryReason::RegistryRead(registry_error)),
    );
    assert_eq!(registry.reads, 1);
    assert_eq!(clock.reads, 0);
    assert_eq!(operation_ids.next_sequence(), 0);
    assert_eq!(job_ids.next_sequence(), 0);
}

#[test]
fn clock_failure_requires_recovery_without_allocating_or_dispatching() {
    let clock_error = BoundaryError::Clock("monotonic clock unavailable".to_string());
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")]);
    let mut clock = FixedClock::error(clock_error.clone());
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
    .expect_err("clock recovery");

    assert_eq!(
        err,
        Phase2BootRunError::RecoveryRequired(Phase2RecoveryReason::Clock(clock_error)),
    );
    assert_eq!(registry.reads, 1);
    assert_eq!(clock.reads, 1);
    assert_eq!(operation_ids.next_sequence(), 0);
    assert_eq!(job_ids.next_sequence(), 0);
    assert!(operations.active_for_service("app").is_empty());
}
