use crate::boot::BootMode;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::service::{ErrorControl, ServiceDefinition};

use super::super::SafeModeDowngrade;
use super::super::planner::prepare_phase2_boot_plan_with_retained;
use super::super::prepare_phase2_boot_plan;
use super::OBSERVED_AT_NS;

#[test]
fn safe_mode_drops_dependencies_that_are_not_safe_mode_eligible() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.safe_mode = true;
    app.requires.push("registryd".to_string());
    let mut registryd = ServiceDefinition::simple_system_boot("registryd", "/sbin/registryd");
    registryd.triggers.clear();
    let normal = ServiceDefinition::simple_system_boot("normal", "/sbin/normal");

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan(
        BootMode::Safe,
        &[app, registryd, normal],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("safe boot plan");

    let services = boot
        .starts
        .iter()
        .map(|start| start.service.as_str())
        .collect::<Vec<_>>();
    assert_eq!(services, vec!["app"]);
    assert!(boot.blocked.is_empty());
}

#[test]
fn safe_mode_starts_critical_boot_roots_without_safe_mode_flag() {
    let mut critical = ServiceDefinition::simple_system_boot("critical", "/sbin/critical");
    critical.error_control = ErrorControl::Critical;
    let normal = ServiceDefinition::simple_system_boot("normal", "/sbin/normal");

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan(
        BootMode::Safe,
        &[critical, normal],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("safe boot plan");

    let services = boot
        .starts
        .iter()
        .map(|start| start.service.as_str())
        .collect::<Vec<_>>();
    assert_eq!(services, vec!["critical"]);
    assert!(boot.blocked.is_empty());
}

#[test]
fn critical_cycle_in_full_boot_downgrades_to_safe_mode() {
    let mut critical = ServiceDefinition::simple_system_boot("critical", "/sbin/critical");
    critical.error_control = ErrorControl::Critical;
    critical.requires.push("normal".to_string());
    let mut normal = ServiceDefinition::simple_system_boot("normal", "/sbin/normal");
    normal.requires.push("critical".to_string());

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan(
        BootMode::Full,
        &[critical, normal],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("safe-mode downgraded boot plan");

    let services = boot
        .starts
        .iter()
        .map(|start| start.service.as_str())
        .collect::<Vec<_>>();
    assert_eq!(boot.mode, BootMode::Safe);
    assert_eq!(services, vec!["critical"]);
    // Still empty, and deliberately so: Safe mode was never going to start
    // these, and marking them Failed would claim something about their own
    // health. The reason lives at boot level instead.
    assert!(boot.blocked.is_empty());
    assert_eq!(
        boot.safe_mode_downgrade,
        vec![SafeModeDowngrade::CriticalCycle {
            services: vec!["critical".to_string(), "normal".to_string()],
        }],
    );
}

#[test]
fn critical_boot_conflict_in_full_boot_downgrades_to_safe_mode() {
    let mut critical = ServiceDefinition::simple_system_boot("critical", "/sbin/critical");
    critical.error_control = ErrorControl::Critical;
    critical.conflicts.push("normal".to_string());
    let normal = ServiceDefinition::simple_system_boot("normal", "/sbin/normal");

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan(
        BootMode::Full,
        &[critical, normal],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("safe-mode downgraded boot plan");

    let services = boot
        .starts
        .iter()
        .map(|start| start.service.as_str())
        .collect::<Vec<_>>();
    assert_eq!(boot.mode, BootMode::Safe);
    assert_eq!(services, vec!["critical"]);
    assert!(boot.blocked.is_empty());
    assert_eq!(
        boot.safe_mode_downgrade,
        vec![SafeModeDowngrade::CriticalBootConflict {
            service: "critical".to_string(),
            target: "normal".to_string(),
        }],
    );
}

/// A boot that was not downgraded carries no downgrade findings, so an empty
/// vec is a reliable "this was a Full boot" rather than merely "nobody looked".
#[test]
fn a_full_boot_records_no_safe_mode_downgrade() {
    let app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
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

    assert_eq!(boot.mode, BootMode::Full);
    assert!(boot.safe_mode_downgrade.is_empty());
}

/// Both kinds of finding are reported when both are present -- the downgrade
/// is not a single reason, and an operator fixing only the one they were shown
/// would reboot into the same Safe mode.
#[test]
fn every_downgrade_finding_is_reported_not_just_the_first() {
    let mut cycle_a = ServiceDefinition::simple_system_boot("cycle-a", "/sbin/cycle-a");
    cycle_a.error_control = ErrorControl::Critical;
    cycle_a.requires.push("cycle-b".to_string());
    let mut cycle_b = ServiceDefinition::simple_system_boot("cycle-b", "/sbin/cycle-b");
    cycle_b.requires.push("cycle-a".to_string());

    let mut clash = ServiceDefinition::simple_system_boot("clash", "/sbin/clash");
    clash.error_control = ErrorControl::Critical;
    clash.conflicts.push("other".to_string());
    let other = ServiceDefinition::simple_system_boot("other", "/sbin/other");

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan(
        BootMode::Full,
        &[cycle_a, cycle_b, clash, other],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("safe-mode downgraded boot plan");

    assert_eq!(boot.mode, BootMode::Safe);
    assert!(
        boot.safe_mode_downgrade
            .iter()
            .any(|finding| matches!(finding, SafeModeDowngrade::CriticalBootConflict { .. })),
        "{:?}",
        boot.safe_mode_downgrade,
    );
    assert!(
        boot.safe_mode_downgrade
            .iter()
            .any(|finding| matches!(finding, SafeModeDowngrade::CriticalCycle { .. })),
        "{:?}",
        boot.safe_mode_downgrade,
    );
}

#[test]
fn retained_phase1_registryd_is_not_started_again() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.requires.push("registryd".to_string());
    let mut registryd = ServiceDefinition::compiled_in_registryd();
    registryd.triggers.clear();

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan_with_retained(
        BootMode::Full,
        &[registryd, app],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
        &["registryd".to_string()],
    )
    .expect("retained boot plan");

    let services = boot
        .starts
        .iter()
        .map(|start| start.service.as_str())
        .collect::<Vec<_>>();
    assert_eq!(services, vec!["app"]);
}
