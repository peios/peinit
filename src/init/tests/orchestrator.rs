use crate::boot::BootMode;
use crate::boundary::BoundaryError;
use crate::init::{
    DeviceNodePolicyFailure, DeviceNodePolicyReport, InitConfig, InitFatalError,
    InitRecoveryReason, InitRunError, InitRunResult, KernelCommandLine, MachineIdStatus,
    Phase1Infrastructure, Phase1InfrastructureWarning, QuietLevel, run_init,
};
use crate::provisioning::{
    ProvisionedPath, ProvisionedPathApplyFailure, ProvisionedPathApplyReport, ProvisionedPathKind,
    ProvisionedPathRegistrySnapshot, ProvisionedPathSecurity,
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
        boot_attempt_threshold: None,
        notify_socket_path: None,
        quiet: QuietLevel::default(),
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
            boot_attempt_threshold: None,
            notify_socket_path: None,
            quiet: QuietLevel::default(),
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
fn device_node_policy_failures_are_warnings_not_recovery() {
    let report = DeviceNodePolicyReport {
        applied: vec!["/dev/zero".to_string()],
        missing: vec!["/dev/full".to_string()],
        failures: vec![DeviceNodePolicyFailure {
            name: "DevNull".to_string(),
            path: "/dev/null".to_string(),
            message: "boom".to_string(),
        }],
    };
    let mut platform = Platform::new().device_policy_report(report);
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
    assert!(platform.recovery_reasons.is_empty());
    assert!(
        platform.console_messages.iter().any(|message| {
            message == "peinit warning: device node /dev/null (DevNull) descriptor failed: boom\n"
        }),
        "{:?}",
        platform.console_messages,
    );
}

#[test]
fn device_node_policy_boundary_error_is_warning_not_recovery() {
    let mut platform = Platform::new().device_policy_error("no /dev");
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
    assert!(platform.recovery_reasons.is_empty());
    assert!(
        platform.console_messages.iter().any(|message| {
            message == "peinit warning: device node policy failed: Recovery(\"no /dev\")\n"
        }),
        "{:?}",
        platform.console_messages,
    );
}

#[test]
fn random_seed_restore_failure_is_warning_not_recovery() {
    let mut platform = Platform::new().random_seed_error("credit failed");
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
    assert!(platform.recovery_reasons.is_empty());
    assert!(
        platform.console_messages.iter().any(|message| {
            message == "peinit warning: random seed restore failed: Recovery(\"credit failed\")\n"
        }),
        "{:?}",
        platform.console_messages,
    );
}

#[test]
fn restored_random_seed_is_logged_before_registryd_start() {
    let mut platform = Platform::new().random_seed_restored();
    let mut registry = Registry::with_services([service("app")]);
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

    let restored_index = platform
        .console_messages
        .iter()
        .position(|message| message == "peinit: phase1 restored random seed\n")
        .expect("restored message");
    let registryd_index = platform
        .console_messages
        .iter()
        .position(|message| message == "peinit: phase1 starting registryd\n")
        .expect("registryd message");
    assert!(restored_index < registryd_index);
}

#[test]
fn generated_machine_id_is_logged_before_registryd_start() {
    let mut platform = Platform::new().machine_id_status(MachineIdStatus::Generated);
    let mut registry = Registry::with_services([service("app")]);
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

    let generated_index = platform
        .console_messages
        .iter()
        .position(|message| message == "peinit: phase1 generated machine-id\n")
        .expect("machine-id message");
    let registryd_index = platform
        .console_messages
        .iter()
        .position(|message| message == "peinit: phase1 starting registryd\n")
        .expect("registryd message");
    assert!(generated_index < registryd_index);
}

#[test]
fn invalid_machine_id_replacement_is_logged_before_registryd_start() {
    let mut platform = Platform::new().machine_id_status(MachineIdStatus::ReplacedInvalid);
    let mut registry = Registry::with_services([service("app")]);
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

    let warning_index = platform
        .console_messages
        .iter()
        .position(|message| message == "peinit warning: invalid machine-id replaced\n")
        .expect("machine-id warning");
    let registryd_index = platform
        .console_messages
        .iter()
        .position(|message| message == "peinit: phase1 starting registryd\n")
        .expect("registryd message");
    assert!(warning_index < registryd_index);
}

#[test]
fn machine_id_failure_enters_recovery_before_registryd_start() {
    let mut platform = Platform::new().machine_id_error("write failed");
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
    .expect("recovery");

    assert!(matches!(
        result,
        InitRunResult::RecoveryReturned {
            reason: InitRecoveryReason::MachineId(_),
        },
    ));
    assert!(!runtime.entered);
    assert!(
        !platform
            .console_messages
            .iter()
            .any(|message| message == "peinit: phase1 starting registryd\n"),
    );
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

/// `peios.bootattempts=N` overrides the compiled-in threshold. It has to be a
/// command-line value rather than a registry one: the check runs in Phase 1,
/// before registryd is launched, so there is nothing to read it from.
#[test]
fn command_line_boot_attempt_threshold_overrides_the_default() {
    let mut platform = Platform::new()
        .boot_attempt_counter(3)
        .command_line(KernelCommandLine {
            boot_attempt_threshold: Some(10),
            ..KernelCommandLine::default()
        });
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

    // 3 attempts against a threshold of 10 is a normal boot, where the
    // compiled-in threshold of 3 would have diverted into recovery.
    assert_eq!(result, InitRunResult::RuntimeReturned);
    assert!(runtime.entered);
}

/// The escape hatch: a system whose boot-attempt counter is itself the problem
/// (a root that reports failure but boots fine) can be told to stop counting.
#[test]
fn command_line_boot_attempt_threshold_of_zero_disables_the_check() {
    let mut platform =
        Platform::new()
            .boot_attempt_counter(9_999)
            .command_line(KernelCommandLine {
                boot_attempt_threshold: Some(0),
                ..KernelCommandLine::default()
            });
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
}

/// `peios.notifysocket=PATH` must be applied before registryd is launched,
/// because binding the socket is the first thing that launch does.
#[test]
fn command_line_notify_socket_path_is_bound_before_registryd_starts() {
    let mut platform = Platform::new().command_line(KernelCommandLine {
        notify_socket_path: Some("/run/alt/notify.sock".to_string()),
        ..KernelCommandLine::default()
    });
    let mut registry = Registry::with_services([service("app")]);
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

    assert_eq!(
        platform.registryd_notify_socket_path.as_deref(),
        Some("/run/alt/notify.sock"),
    );
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
    let mut platform = Platform::new();
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
fn provisioned_paths_are_applied_after_registryd_before_phase2() {
    let entry = provisioned_directory("eventd-run", "/run/services/eventd", false);
    let mut snapshot = ProvisionedPathRegistrySnapshot::empty();
    snapshot.entries.push(entry.clone());
    let mut platform = Platform::new();
    let mut registry = Registry::with_services([service("app")]).with_provisioned_paths(snapshot);
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
    assert_eq!(platform.provisioned_paths_seen, vec![entry]);
    let provision_index = platform
        .console_messages
        .iter()
        .position(|message| message == "peinit: phase2 boot starting\n")
        .expect("phase2 message");
    let registryd_index = platform
        .console_messages
        .iter()
        .position(|message| message == "peinit: phase1 registryd started\n")
        .expect("registryd message");
    assert!(registryd_index < provision_index);
}

#[test]
fn malformed_provisioned_path_entries_are_warnings_not_recovery() {
    let mut platform = Platform::new();
    let mut registry = Registry::with_services([service("app")])
        .provisioned_path_registry_warning("broken", "missing Path");
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
    assert!(
        platform.console_messages.iter().any(|message| {
            message == "peinit warning: provisioned path broken ignored: missing Path\n"
        }),
        "{:?}",
        platform.console_messages,
    );
}

#[test]
fn optional_provisioned_path_failures_are_warnings_not_recovery() {
    let entry = provisioned_directory("cache", "/run/cache", false);
    let mut snapshot = ProvisionedPathRegistrySnapshot::empty();
    snapshot.entries.push(entry);
    let mut report = ProvisionedPathApplyReport::default();
    report.warnings.push(ProvisionedPathApplyFailure {
        entry: "cache".to_string(),
        path: "/run/cache".to_string(),
        message: "permission denied".to_string(),
    });
    let mut platform = Platform::new().provision_report(report);
    let mut registry = Registry::with_services([service("app")]).with_provisioned_paths(snapshot);
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
    assert!(platform.recovery_reasons.is_empty());
    assert!(
        platform.console_messages.iter().any(|message| {
            message
                == "peinit warning: provisioned path cache at /run/cache failed: permission denied\n"
        }),
        "{:?}",
        platform.console_messages,
    );
}

#[test]
fn required_provisioned_path_failures_enter_recovery_before_phase2() {
    let entry = provisioned_directory("eventd", "/run/services/eventd", true);
    let mut snapshot = ProvisionedPathRegistrySnapshot::empty();
    snapshot.entries.push(entry);
    let mut report = ProvisionedPathApplyReport::default();
    report.required_failures.push(ProvisionedPathApplyFailure {
        entry: "eventd".to_string(),
        path: "/run/services/eventd".to_string(),
        message: "security rejected".to_string(),
    });
    let mut platform = Platform::new().provision_report(report);
    let mut registry = Registry::with_services([service("app")]).with_provisioned_paths(snapshot);
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
            reason: InitRecoveryReason::Provisioning(_),
        },
    ));
    assert!(!runtime.entered);
    assert!(
        !platform
            .console_messages
            .iter()
            .any(|message| message == "peinit: phase2 boot starting\n")
    );
}

#[test]
fn phase1_infrastructure_warnings_are_logged_and_do_not_block_runtime() {
    let mut infrastructure = Phase1Infrastructure::new();
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
        vec![Phase1InfrastructureWarning::LoopbackBringUp {
            interface: "lo".to_string(),
            message: "netlink failed".to_string(),
        },],
    );
}

fn provisioned_directory(name: &str, path: &str, required: bool) -> ProvisionedPath {
    ProvisionedPath {
        name: name.to_string(),
        kind: ProvisionedPathKind::Directory,
        path: path.to_string(),
        security: ProvisionedPathSecurity::Default,
        required,
    }
}

#[test]
fn safemode_flag_sets_supervisor_boot_mode() {
    let mut platform = Platform::new().command_line(KernelCommandLine {
        safe_mode: true,
        recovery: false,
        boot_attempt_threshold: None,
        notify_socket_path: None,
        quiet: QuietLevel::default(),
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
