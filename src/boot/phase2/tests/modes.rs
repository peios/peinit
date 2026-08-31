use crate::boot::BootMode;
use crate::boundary::UndecodableService;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::service::runtime::TransitionCause;
use crate::service::{ErrorControl, ServiceDefinition};

use super::super::SafeModeDowngrade;
use super::super::planner::{Phase2PlanContext, prepare_phase2_boot_plan_with_retained};
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
        Phase2PlanContext {
            retained_satisfied: &["registryd".to_string()],
            ..Default::default()
        },
    )
    .expect("retained boot plan");

    let services = boot
        .starts
        .iter()
        .map(|start| start.service.as_str())
        .collect::<Vec<_>>();
    assert_eq!(services, vec!["app"]);
}

#[test]
fn an_undecodable_definition_fails_only_that_service() {
    // PSD-007 §2.2: "Service definition fails validation | Service marked
    // Failed (ValidationError). Other services continue."
    //
    // The registry read used to propagate the first decode error, which became
    // BoundaryError::Registry -> Phase2RecoveryReason::RegistryRead and took
    // the machine to the recovery console. One typo in one service key bricked
    // the next boot.
    let healthy = ServiceDefinition::simple_system_boot("healthy", "/sbin/healthy");
    let other = ServiceDefinition::simple_system_boot("other", "/sbin/other");
    let undecodable = [UndecodableService {
        name: "broken".to_string(),
        message: "unclosed double quote in ImagePath".to_string(),
    }];

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan_with_retained(
        BootMode::Full,
        &[healthy, other],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
        Phase2PlanContext {
            undecodable: &undecodable,
            ..Default::default()
        },
    )
    .expect("a bad definition must not fail the whole plan");

    assert_eq!(
        boot.starts
            .iter()
            .map(|start| start.service.as_str())
            .collect::<Vec<_>>(),
        vec!["healthy", "other"],
    );
    assert_eq!(boot.blocked.len(), 1);
    assert_eq!(boot.blocked[0].service, "broken");
    assert_eq!(
        boot.blocked[0].reason.transition_cause(),
        TransitionCause::ValidationError,
        "the spec's per-definition outcome, and the cause the state machine already has",
    );
}

#[test]
fn a_dependent_of_an_undecodable_definition_fails_through_dependency_failure() {
    // The blast radius is bounded by what actually depended on it, rather than
    // by the whole machine.
    let mut dependent = ServiceDefinition::simple_system_boot("dependent", "/sbin/dependent");
    dependent.requires.push("broken".to_string());
    let unrelated = ServiceDefinition::simple_system_boot("unrelated", "/sbin/unrelated");
    let undecodable = [UndecodableService {
        name: "broken".to_string(),
        message: "invalid service name".to_string(),
    }];

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan_with_retained(
        BootMode::Full,
        &[dependent, unrelated],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
        Phase2PlanContext {
            undecodable: &undecodable,
            ..Default::default()
        },
    )
    .expect("boot plan");

    assert_eq!(
        boot.starts
            .iter()
            .map(|start| start.service.as_str())
            .collect::<Vec<_>>(),
        vec!["unrelated"],
        "only the dependent is lost",
    );
    let causes = boot
        .blocked
        .iter()
        .map(|blocked| (blocked.service.as_str(), blocked.reason.transition_cause()))
        .collect::<Vec<_>>();
    assert!(causes.contains(&("broken", TransitionCause::ValidationError)));
    assert!(causes.contains(&("dependent", TransitionCause::DependencyFailure)));
}

// PEI-343. Safe mode's own rule is that dependencies on *excluded* services
// are dropped (§2.3) — otherwise excluding a service would fail everything
// downstream of it and Safe mode could start almost nothing. The guard
// implementing that was on the whole unavailable case, so it also dropped
// hard dependencies on targets that are missing from the registry or disabled
// by an administrator.
//
// Those two are configuration errors, not Safe mode exclusions, and Safe mode
// exists precisely because the configuration is already known to be broken. So
// the cautious mode was the one starting a service without the thing its
// `Requires` — the strongest statement a definition can make about what it
// needs — says it needs, on the escalation path a Full boot had already failed.
#[test]
fn safe_mode_blocks_a_hard_dependency_missing_from_the_registry() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.safe_mode = true;
    app.requires.push("absent".to_string());

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan(
        BootMode::Safe,
        &[app],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("safe boot plan");

    assert!(boot.starts.is_empty());
    assert_eq!(boot.blocked.len(), 1);
    assert_eq!(boot.blocked[0].service, "app");
    assert_eq!(
        boot.blocked[0].reason,
        crate::boot::phase2::BlockedReason::HardDependencyUnavailable {
            target: "absent".to_string(),
            kind: crate::boot::phase2::DependencyKind::Requires,
        },
    );
}

// PEI-343, the disabled half. Full mode has blocked this since it existed
// (`disabled_services_are_not_auto_started_and_hard_dependents_are_blocked`);
// Safe mode did not.
#[test]
fn safe_mode_blocks_a_disabled_hard_dependency() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.safe_mode = true;
    app.requires.push("db".to_string());
    // Eligible in every respect except that an administrator turned it off.
    let mut db = ServiceDefinition::simple_system_boot("db", "/sbin/db");
    db.safe_mode = true;
    db.disabled = true;

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan(
        BootMode::Safe,
        &[app, db],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("safe boot plan");

    assert!(boot.starts.is_empty());
    assert_eq!(boot.blocked.len(), 1);
    assert_eq!(boot.blocked[0].service, "app");
    assert_eq!(
        boot.blocked[0].reason,
        crate::boot::phase2::BlockedReason::HardDependencyUnavailable {
            target: "db".to_string(),
            kind: crate::boot::phase2::DependencyKind::Requires,
        },
    );
}

// A `BindsTo` target that is missing is treated as a missing `Requires`
// (§6.1), so the same rule reaches it.
#[test]
fn safe_mode_blocks_a_missing_binds_to_target() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.safe_mode = true;
    app.binds_to.push("absent".to_string());

    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let boot = prepare_phase2_boot_plan(
        BootMode::Safe,
        &[app],
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("safe boot plan");

    assert!(boot.starts.is_empty());
    assert_eq!(boot.blocked.len(), 1);
    assert_eq!(
        boot.blocked[0].reason,
        crate::boot::phase2::BlockedReason::HardDependencyUnavailable {
            target: "absent".to_string(),
            kind: crate::boot::phase2::DependencyKind::BindsTo,
        },
    );
}
