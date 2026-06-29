use crate::boot::BootMode;
use crate::boundary::BoundaryError;
use crate::init::{
    InitConfig, InitFatalError, InitRecoveryReason, InitRunError, InitRunResult, KernelCommandLine,
    Phase1Infrastructure, Phase1InfrastructureWarning, run_init,
};
use crate::service::runtime::ServiceState;

use super::support::{ClockAt, Platform, Registry, Runtime, critical_service, service};

#[test]
fn non_pid1_is_fatal_and_does_not_enter_recovery() {
    let mut platform = Platform::new().not_pid1(44);
    let mut registry = Registry::with_services([]);
    let mut clock = ClockAt(1);
    let mut runtime = Runtime::default();

    let err = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect_err("not pid1");

    assert_eq!(
        err,
        InitRunError::Fatal(InitFatalError::NotPid1 { pid: 44 }),
    );
    assert!(platform.recovery_reasons.is_empty());
    assert!(!runtime.entered);
}

#[test]
fn recovery_flag_enters_recovery_after_root_probe_and_counter_increment() {
    let mut platform = Platform::new().command_line(KernelCommandLine {
        recovery: true,
        safe_mode: false,
        console: false,
    });
    let mut registry = Registry::with_services([]);
    let mut clock = ClockAt(1);
    let mut runtime = Runtime::default();

    let result = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("recovery");

    assert_eq!(
        result,
        InitRunResult::RecoveryReturned {
            reason: InitRecoveryReason::ForcedByKernelCommandLine,
        },
    );
    assert!(platform.root_verified);
    assert_eq!(platform.increment_calls, 1);
    assert!(!runtime.entered);
}

#[test]
fn recovery_flag_does_not_require_readable_boot_attempt_counter() {
    let mut platform = Platform::new()
        .command_line(KernelCommandLine {
            recovery: true,
            safe_mode: false,
            console: false,
        })
        .boot_attempt_counter_error("malformed counter");
    let mut registry = Registry::with_services([]);
    let mut clock = ClockAt(1);
    let mut runtime = Runtime::default();

    let result = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("recovery");

    assert_eq!(
        result,
        InitRunResult::RecoveryReturned {
            reason: InitRecoveryReason::ForcedByKernelCommandLine,
        },
    );
    assert_eq!(platform.increment_calls, 1);
    assert!(!runtime.entered);
}

#[test]
fn kernel_command_line_is_read_after_virtual_filesystem_mounts() {
    let mut platform = Platform::new().command_line_error("missing /proc/cmdline");
    let mut registry = Registry::with_services([]);
    let mut clock = ClockAt(1);
    let mut runtime = Runtime::default();

    let result = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("recovery");

    assert!(matches!(
        result,
        InitRunResult::RecoveryReturned {
            reason: InitRecoveryReason::KernelCommandLine(_),
        },
    ));
    let mounted_index = platform
        .console_messages
        .iter()
        .position(|message| message == "peinit: phase1 virtual filesystems mounted\n")
        .expect("mounted message");
    let recovery_index = platform
        .console_messages
        .iter()
        .position(|message| message.starts_with("peinit: entering recovery: KernelCommandLine("))
        .expect("recovery message");
    assert!(mounted_index < recovery_index);
}

#[test]
fn boot_attempt_threshold_enters_recovery() {
    let mut platform = Platform::new().boot_attempt_counter(3);
    let mut registry = Registry::with_services([]);
    let mut clock = ClockAt(1);
    let mut runtime = Runtime::default();

    let result = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("recovery");

    assert_eq!(
        result,
        InitRunResult::RecoveryReturned {
            reason: InitRecoveryReason::BootAttemptThresholdReached {
                counter: 3,
                threshold: 3,
            },
        },
    );
    assert!(!runtime.entered);
}

#[test]
fn boot_attempt_increment_failure_treats_counter_as_zero_and_continues() {
    let mut platform = Platform::new()
        .boot_attempt_counter(3)
        .increment_boot_attempt_counter_error("disk full");
    let mut registry = Registry::with_services([service("app")]);
    let mut clock = ClockAt(10);
    let mut runtime = Runtime::default();

    let result = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("runtime");

    assert_eq!(result, InitRunResult::RuntimeReturned);
    assert_eq!(platform.increment_calls, 1);
    assert!(platform.recovery_reasons.is_empty());
    assert!(runtime.entered);
}

#[test]
fn successful_boot_enters_runtime_with_phase2_booted_supervisor() {
    let mut platform = Platform::new().with_jfs_infrastructure();
    let mut registry = Registry::with_services([service("app")]);
    let mut clock = ClockAt(10);
    let mut runtime = Runtime::default();

    let result = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("runtime");

    assert_eq!(result, InitRunResult::RuntimeReturned);
    assert!(runtime.entered);
    assert!(runtime.received_jfs);
    assert_eq!(runtime.supervisor_mode, Some(BootMode::Full));
    assert!(runtime.service_names.contains(&"app".to_string()));
    assert_eq!(
        platform.console_messages,
        vec![
            "peinit: phase1 starting\n",
            "peinit: phase1 mounting virtual filesystems\n",
            "peinit: phase1 virtual filesystems mounted\n",
            "peinit: phase1 starting registryd\n",
            "peinit: phase1 registryd started\n",
            "peinit: phase2 boot starting\n",
            "peinit: phase2 boot complete\n",
        ],
    );
}

#[test]
fn phase1_infrastructure_warnings_are_logged_and_do_not_block_runtime() {
    let mut infrastructure = Phase1Infrastructure::new();
    infrastructure.push_warning(Phase1InfrastructureWarning::JfsDeviceOpen {
        path: "/dev/jfs".to_string(),
        message: "missing".to_string(),
    });
    infrastructure.push_warning(Phase1InfrastructureWarning::LoopbackBringUp {
        interface: "lo".to_string(),
        message: "netlink failed".to_string(),
    });
    let mut platform = Platform::new().infrastructure(infrastructure);
    let mut registry = Registry::with_services([service("app")]);
    let mut clock = ClockAt(10);
    let mut runtime = Runtime::default();

    let result = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("runtime");

    assert_eq!(result, InitRunResult::RuntimeReturned);
    assert!(runtime.entered);
    assert_eq!(
        platform.warning_logs,
        vec![
            Phase1InfrastructureWarning::JfsDeviceOpen {
                path: "/dev/jfs".to_string(),
                message: "missing".to_string(),
            },
            Phase1InfrastructureWarning::LoopbackBringUp {
                interface: "lo".to_string(),
                message: "netlink failed".to_string(),
            },
        ],
    );
}

#[test]
fn safemode_flag_sets_supervisor_boot_mode() {
    let mut platform = Platform::new().command_line(KernelCommandLine {
        safe_mode: true,
        recovery: false,
        console: false,
    });
    let mut registry = Registry::with_services([critical_service("core"), service("app")]);
    let mut clock = ClockAt(10);
    let mut runtime = Runtime::default();

    run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("runtime");

    assert_eq!(runtime.supervisor_mode, Some(BootMode::Safe));
    assert_eq!(runtime.state_of("core"), Some(ServiceState::Starting));
    assert_eq!(runtime.state_of("app"), Some(ServiceState::Inactive));
}

#[test]
fn runtime_failure_enters_recovery() {
    let mut platform = Platform::new();
    let mut registry = Registry::with_services([service("app")]);
    let mut clock = ClockAt(10);
    let mut runtime = Runtime::default().fail_runtime();

    let result = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("recovery");

    assert!(matches!(
        result,
        InitRunResult::RecoveryReturned {
            reason: InitRecoveryReason::Runtime(BoundaryError::Recovery(_)),
        },
    ));
    assert!(
        platform
            .console_messages
            .iter()
            .any(|message| message.starts_with("peinit: entering recovery: Runtime(")),
    );
}

#[cfg(feature = "peios-boundary")]
#[test]
fn phase2_graph_failure_emits_recovery_and_graph_audit_events() {
    let a = service("dup");
    let b = service("dup");
    let mut platform = Platform::new();
    let mut registry = Registry::with_services([a, b]);
    let mut clock = ClockAt(10);
    let mut runtime = Runtime::default();

    let result = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("recovery");

    assert!(matches!(
        result,
        InitRunResult::RecoveryReturned {
            reason: InitRecoveryReason::Phase2(_),
        },
    ));
    assert_eq!(
        platform
            .kmes_events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["recovery.entered", "graph.validation_error"],
    );
}

#[cfg(feature = "peios-boundary")]
#[test]
fn noncritical_phase2_cycle_enters_runtime_with_failed_services() {
    let mut a = service("a");
    a.requires.push("b".to_string());
    let mut b = service("b");
    b.triggers.clear();
    b.requires.push("a".to_string());
    let mut platform = Platform::new();
    let mut registry = Registry::with_services([a, b]);
    let mut clock = ClockAt(10);
    let mut runtime = Runtime::default();

    let result = run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .expect("runtime");

    assert_eq!(result, InitRunResult::RuntimeReturned);
    assert_eq!(runtime.state_of("a"), Some(ServiceState::Failed));
    assert_eq!(runtime.state_of("b"), Some(ServiceState::Failed));
    assert!(
        platform
            .console_messages
            .contains(&"peinit: service a failed: CycleDetected\n".to_string())
    );
    assert!(
        platform
            .console_messages
            .contains(&"peinit: service b failed: CycleDetected\n".to_string())
    );
}
