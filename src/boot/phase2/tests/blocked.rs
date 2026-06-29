use crate::boot::BootMode;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::service::{ServiceDefinition, ServiceTrigger};

use super::super::{BlockedReason, DependencyKind, prepare_phase2_boot_plan};
use super::OBSERVED_AT_NS;

#[test]
fn disabled_services_are_not_auto_started_and_hard_dependents_are_blocked() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.requires.push("disabled-db".to_string());
    let mut disabled = ServiceDefinition::simple_system_boot("disabled-db", "/sbin/db");
    disabled.disabled = true;
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();

    let boot = prepare_phase2_boot_plan(
        BootMode::Full,
        &[app, disabled],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("boot plan");

    assert!(boot.starts.is_empty());
    assert_eq!(boot.blocked.len(), 1);
    assert_eq!(boot.blocked[0].service, "app");
    assert_eq!(operations.next_sequence(), 1);
    assert_eq!(jobs.next_sequence(), 0);
    assert_eq!(
        boot.blocked[0].reason,
        BlockedReason::HardDependencyUnavailable {
            target: "disabled-db".to_string(),
            kind: DependencyKind::Requires,
        }
    );
}

#[test]
fn boot_triggered_conflicts_are_blocked_as_validation_errors() {
    let mut a = ServiceDefinition::simple_system_boot("a", "/sbin/a");
    a.conflicts.push("b".to_string());
    let b = ServiceDefinition::simple_system_boot("b", "/sbin/b");
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
    .expect("boot plan");

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
    assert_eq!(
        boot.blocked[0].reason,
        BlockedReason::ConflictingBootService {
            target: "b".to_string(),
        },
    );
    assert_eq!(
        boot.blocked[1].reason,
        BlockedReason::ConflictingBootService {
            target: "a".to_string(),
        },
    );
}

#[test]
fn invalid_health_check_timing_blocks_service_as_validation_error() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.health_check = Some("/bin/check-app".to_string());
    app.health_check_retries = 4;
    app.health_check_interval_secs = 30;
    app.restart_window_secs = 120;
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();

    let boot = prepare_phase2_boot_plan(
        BootMode::Full,
        &[app],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("boot plan");

    assert!(boot.starts.is_empty());
    assert_eq!(boot.blocked.len(), 1);
    assert_eq!(boot.blocked[0].service, "app");
    assert!(matches!(
        boot.blocked[0].reason,
        BlockedReason::ValidationError { .. }
    ));
}

#[test]
fn invalid_timer_schedule_blocks_service_even_when_demand_only() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.triggers = vec![ServiceTrigger::Timer {
        schedule: "*-*-* 12:00:00.5 UTC".to_string(),
    }];
    let ready = ServiceDefinition::simple_system_boot("ready", "/sbin/ready");
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();

    let boot = prepare_phase2_boot_plan(
        BootMode::Full,
        &[app, ready],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("boot plan");

    assert_eq!(
        boot.starts
            .iter()
            .map(|start| start.service.as_str())
            .collect::<Vec<_>>(),
        vec!["ready"],
    );
    assert_eq!(boot.blocked.len(), 1);
    assert_eq!(boot.blocked[0].service, "app");
    let BlockedReason::ValidationError { message } = &boot.blocked[0].reason else {
        panic!("expected validation error");
    };
    assert!(message.contains("Timer schedule"));
    assert_eq!(jobs.next_sequence(), 1);
    assert_eq!(operations.next_sequence(), 2);
}
