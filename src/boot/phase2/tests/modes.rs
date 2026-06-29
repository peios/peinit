use crate::boot::BootMode;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::service::{ErrorControl, ServiceDefinition};

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
    assert!(boot.blocked.is_empty());
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
