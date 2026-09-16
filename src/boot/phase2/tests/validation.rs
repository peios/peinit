use crate::boot::BootMode;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::service::{Readiness, ServiceDefinition, ServiceGraphWarning};

use super::super::{BlockedReason, Phase2BootPlanError, prepare_phase2_boot_plan};
use super::OBSERVED_AT_NS;

#[test]
fn a_blocked_service_does_not_silence_the_graph_warnings() {
    // PEI-1124: the plan blocks `orphan` for its missing dependency and
    // still carries the warning about `db`, which is about to start and
    // release `app` on an Alive promise.
    let mut orphan = ServiceDefinition::simple_system_boot("orphan", "/sbin/orphan");
    orphan.requires.push("missing".to_string());
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.requires.push("db".to_string());
    let mut db = ServiceDefinition::simple_system_boot("db", "/sbin/db");
    db.readiness = Readiness::Alive;
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();

    let boot = prepare_phase2_boot_plan(
        BootMode::Full,
        &[orphan, app, db],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("boot plan with a blocked orphan");

    assert_eq!(
        boot.blocked
            .iter()
            .map(|blocked| blocked.service.as_str())
            .collect::<Vec<_>>(),
        vec!["orphan"],
    );
    assert_eq!(
        boot.warnings,
        vec![ServiceGraphWarning::AliveReadinessWithHardDependents {
            service: "db".to_string(),
            dependents: vec!["app".to_string()],
        }]
    );
}

#[test]
fn zero_parallel_start_limit_is_rejected() {
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let service = ServiceDefinition::simple_system_boot("app", "/sbin/app");

    let err = prepare_phase2_boot_plan(
        BootMode::Full,
        &[service],
        0,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect_err("invalid max parallel starts");

    assert_eq!(err, Phase2BootPlanError::InvalidMaxParallelStarts);
    assert_eq!(operations.next_sequence(), 0);
    assert_eq!(jobs.next_sequence(), 0);
}

#[test]
fn noncritical_cycles_block_participants_and_continue_independent_starts() {
    let mut a = ServiceDefinition::simple_system_boot("a", "/sbin/a");
    a.requires.push("b".to_string());
    let mut b = ServiceDefinition::simple_system_boot("b", "/sbin/b");
    b.triggers.clear();
    b.requires.push("a".to_string());
    let ready = ServiceDefinition::simple_system_boot("ready", "/sbin/ready");
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();

    let boot = prepare_phase2_boot_plan(
        BootMode::Full,
        &[a, b, ready],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("boot plan with blocked cycle");

    assert_eq!(
        boot.starts
            .iter()
            .map(|start| start.service.as_str())
            .collect::<Vec<_>>(),
        vec!["ready"],
    );
    assert_eq!(
        boot.blocked
            .iter()
            .map(|blocked| blocked.service.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"],
    );
    assert!(
        boot.blocked
            .iter()
            .all(|blocked| matches!(blocked.reason, BlockedReason::CycleDetected { .. }))
    );
    assert_eq!(operations.next_sequence(), 3);
    assert_eq!(jobs.next_sequence(), 1);
}

#[test]
fn cycle_detected_takes_primary_cause_precedence_over_validation_errors() {
    let mut a = ServiceDefinition::simple_system_boot("a", "/sbin/a");
    a.requires.push("b".to_string());
    a.health_check = Some("/bin/false".to_string());
    a.health_check_retries = 4;
    a.health_check_interval_secs = 30;
    a.restart_window_secs = 120;
    let mut b = ServiceDefinition::simple_system_boot("b", "/sbin/b");
    b.triggers.clear();
    b.requires.push("a".to_string());
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();

    let boot = prepare_phase2_boot_plan(
        BootMode::Full,
        &[a, b],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("boot plan with blocked cycle");

    assert!(boot.starts.is_empty());
    assert_eq!(boot.blocked.len(), 2);
    assert_eq!(boot.blocked[0].service, "a");
    assert!(matches!(
        boot.blocked[0].reason,
        BlockedReason::CycleDetected { .. }
    ));
    // §6.2: precedence picks the primary cause, it does not suppress the rest.
    // `a` also has an invalid health-check configuration, and an administrator
    // who breaks the cycle should not have to reboot to discover that.
    assert!(
        boot.blocked[0]
            .additional_reasons
            .iter()
            .any(|reason| matches!(reason, BlockedReason::ValidationError { .. })),
        "the health-check validation error must be retained beside the cycle: {:?}",
        boot.blocked[0].additional_reasons,
    );
}

#[test]
fn allocation_failure_does_not_partially_advance_ids() {
    let service = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    let mut operations = OperationIdAllocator::with_next_sequence(u64::MAX);
    let mut jobs = JobIdAllocator::new();

    let err = prepare_phase2_boot_plan(
        BootMode::Full,
        &[service],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect_err("operation sequence exhaustion");

    assert!(matches!(err, Phase2BootPlanError::OperationIdAllocation(_)));
    assert_eq!(operations.next_sequence(), u64::MAX);
    assert_eq!(jobs.next_sequence(), 0);
}
