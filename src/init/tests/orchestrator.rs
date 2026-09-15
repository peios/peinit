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
        dumb_terminal: false,
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
            dumb_terminal: false,
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
fn kernel_command_line_is_read_before_any_phase1_work() {
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
    // "phase1 starting" still comes first: it is the evidence peinit ran, and a
    // machine that cannot read its command line is exactly when that matters.
    //
    // Nothing else from Phase 1 should have run. This reverses an earlier invariant
    // ("read the command line AFTER the virtual filesystem mounts"), which
    // assumed mounting is what makes /proc/cmdline readable. It is not:
    // `mount_phase1_virtual_filesystems` reads /proc/self/mountinfo *before* it
    // mounts anything, and /proc is `initramfs_provided` — prelude mount-moves
    // it into the root before exec'ing peinit. A peinit that cannot read
    // /proc/cmdline could not have read mountinfo either, so the old ordering
    // bought nothing and cost the stage banner its place at the stage.
    assert!(
        !platform
            .console_messages
            .iter()
            .any(|message| message.starts_with("peinit: phase1 mounting")),
        "no Phase 1 work should precede the command line: {:?}",
        platform.console_messages
    );
    let recovery_index = platform
        .console_messages
        .iter()
        .position(|message| message.starts_with("peinit: entering recovery: KernelCommandLine("))
        .expect("recovery message");
    let starting_index = platform
        .console_messages
        .iter()
        .position(|message| message == "peinit: phase1 starting\n")
        .expect("starting message");
    assert!(starting_index < recovery_index);
}

/// The tag is what an operator actually scans for, so it is worth asserting
/// separately from the prose. In particular a Phase 1 "warning" must render as
/// `[ WARN ]` and not `[FAILED]`: every one of these sites used the error
/// helper before there were tags, which was invisible then and overstates the
/// problem now.
#[test]
fn phase1_tags_match_what_happened() {
    use crate::console_style::ConsoleTag;

    let mut platform = Platform::new();
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

    let tag_of = |needle: &str| {
        platform
            .console_tags
            .iter()
            .find(|(_, message)| message.contains(needle))
            .map(|(tag, _)| *tag)
            .unwrap_or_else(|| panic!("no message containing {needle:?}"))
    };

    // Outcomes.
    assert_eq!(tag_of("virtual filesystems mounted"), ConsoleTag::Ok);
    assert_eq!(tag_of("registryd started"), ConsoleTag::Ok);
    assert_eq!(tag_of("phase2 boot complete"), ConsoleTag::Ok);
    // Progress with no outcome yet holds the column but stays blank.
    assert_eq!(tag_of("phase1 starting\n"), ConsoleTag::None);
    assert_eq!(tag_of("phase2 boot starting"), ConsoleTag::None);
    // The banner brings its own layout and must not be given a tag column.
    assert_eq!(tag_of("real root · PID 1"), ConsoleTag::Bare);
}

/// A configuration warning is a boot that went on with a fallback value, so
/// it renders as `[ WARN ]`. It used the error helper, and so read as
/// `[FAILED]` for something that had not failed (PEI-809).
#[test]
fn configuration_warnings_are_tagged_warn_not_failed() {
    use crate::console_style::ConsoleTag;

    let mut platform = Platform::new();
    let mut registry = Registry::with_services([service("app")])
        .with_services_schema_version(crate::registry::SUPPORTED_SERVICES_SCHEMA_VERSION + 1);
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

    let (tag, message) = platform
        .console_tags
        .iter()
        .find(|(_, message)| message.contains("services schema version"))
        .expect("the schema warning reached the console");
    assert!(message.starts_with("peinit warning: "), "{message}");
    assert_eq!(*tag, ConsoleTag::Warn);
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
    // The stage banner sits between peinit announcing itself and its first
    // Phase 1 step, and carries the mode this boot started in. Matched by its
    // content rather than spelled out, so a change to the rule character or the
    // width does not fail an unrelated test.
    let banner = platform
        .console_messages
        .iter()
        .find(|message| message.contains("peinit · real root · PID 1 · Full boot"))
        .cloned()
        .expect("stage banner");
    assert_eq!(
        platform.console_messages,
        vec![
            "peinit: phase1 starting\n".to_string(),
            banner,
            "peinit: phase1 mounting virtual filesystems\n".to_string(),
            "peinit: phase1 virtual filesystems mounted\n".to_string(),
            "peinit: phase1 starting registryd\n".to_string(),
            "peinit: phase1 registryd started\n".to_string(),
            "peinit: phase2 boot starting\n".to_string(),
            "peinit: phase2 boot complete\n".to_string(),
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
        dumb_terminal: false,
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

// PEI-337. Recovery entered from a Phase 1 failure used to skip the Phase 1
// catch-up entirely, so a `/dev/shm` or `/sys/fs/cgroup` mount failure handed
// the operator a shell with the random seed unrestored, no machine ID, **the
// clock unset** — every timestamp in the session wrong — and no registryd, so
// every `reg`-family tool failed.
//
// The steps are individually idempotent, so "complete Phase 1 steps 1-5 if not
// already done" is served by running them and ignoring the failures.
#[test]
fn recovery_from_a_phase1_failure_completes_phase1_and_starts_registryd() {
    let mut platform = Platform::new().mount_error("no /sys/fs/cgroup");
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
            reason: InitRecoveryReason::VirtualFilesystems(_),
        },
    ));
    // The mount is retried — it is the step that failed, and by the time the
    // operator has a shell it may well succeed.
    assert_eq!(platform.mount_calls, 2);
    assert_eq!(platform.random_seed_calls, 1);
    assert_eq!(platform.machine_id_calls, 1);
    assert_eq!(platform.rtc_calls, 1);
    assert_eq!(platform.registryd_starts, 1);
}

// PEI-337. The starkest case: nothing about registryd has failed, and the
// operator still used to get no registry.
#[test]
fn recovery_from_an_rtc_failure_still_starts_registryd() {
    let mut platform = Platform::new().rtc_error("no rtc");
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
            reason: InitRecoveryReason::RtcClock(_),
        },
    ));
    assert_eq!(platform.registryd_starts, 1);
}

// PEI-338. The counterpart: where the failure paths did too little, the
// success-adjacent ones did too much. Recovery entered after Phase 1 had
// already started registryd built a fresh supervisor and started a second one.
// `NotifySocket::bind` unlinks the socket path before binding, so the second
// bind succeeds rather than reporting EADDRINUSE, and a second `/sbin/registryd`
// forks against the same loregd hive files — while an operator is trying to work
// out what went wrong.
#[test]
fn recovery_after_registryd_started_does_not_fork_a_second_one() {
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
            reason: InitRecoveryReason::Runtime(_),
        },
    ));
    assert_eq!(platform.registryd_starts, 1);
}

// PEI-338, the second half. The recovery supervisor was built from
// `SupervisorSettings::default()`, discarding the parsed command line, so a
// machine booted with `peios.notifysocket=` got a recovery registryd pointed at
// the default path.
#[test]
fn a_recovery_registryd_uses_the_parsed_notify_socket_path() {
    let mut platform = Platform::new()
        .command_line(KernelCommandLine {
            notify_socket_path: Some("/run/alt/notify.sock".to_string()),
            ..KernelCommandLine::default()
        })
        .rtc_error("no rtc");
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
    .expect("recovery");

    assert_eq!(platform.registryd_starts, 1);
    assert_eq!(
        platform.registryd_notify_socket_path.as_deref(),
        Some("/run/alt/notify.sock"),
    );
}

// PEI-363. The boot continues, loudly, when the identifier cannot be
// persisted. §2.1 step 4 has no recovery path — the failure table says only
// "generate/replace and continue" — and dropping to a recovery shell for a
// missing `/lcl/etc/` on first boot was disproportionate to what the value is:
// "a local opaque install ID" that "MUST NOT be treated as a security
// principal, credential, SID, account, or authorization input".
#[test]
fn an_unpersistable_machine_id_warns_and_boots() {
    let mut platform = Platform::new().machine_id_status(MachineIdStatus::Ephemeral {
        reason: "write /lcl/etc/machine-id failed: No such file or directory".to_string(),
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
    .expect("boot");

    assert!(matches!(result, InitRunResult::RuntimeReturned));
    assert!(runtime.entered);
    assert!(platform.recovery_reasons.is_empty());
    assert!(
        platform.console_messages.iter().any(|message| {
            message.starts_with("peinit warning: machine-id not persisted")
                && message.contains("using an identifier for this boot only")
        }),
        "the operator was told nothing: {:?}",
        platform.console_messages,
    );
}

// PEI-365. peinit never checked that it held the privileges §13.1 says it
// requires. SeCreateTokenPrivilege surfaced only as an EPERM from
// kacs_create_token at the *first* service start — registryd, in Phase 1 step
// 6 — so a peinit that could not mint tokens entered recovery reporting a
// token-materialisation failure that read as a registryd problem, with nothing
// anywhere naming the privilege.
//
// PID 1 discovering it cannot mint tokens is worth saying before it tries.
#[test]
fn missing_privileges_are_named_before_anything_needs_them() {
    let mut platform = Platform::new()
        .missing_privileges("peinit is missing required privilege(s): SeCreateTokenPrivilege");
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
            reason: InitRecoveryReason::Privileges(_),
        },
    ));
    // Before registryd is attempted, so the operator is not left diagnosing a
    // registryd that would never have started.
    assert!(
        !platform
            .console_messages
            .iter()
            .any(|message| message == "peinit: phase1 starting registryd\n"),
    );
    assert!(
        platform.console_messages.iter().any(|message| {
            message.contains("required privileges are not held")
                && message.contains("SeCreateTokenPrivilege")
        }),
        "the missing privilege was not named: {:?}",
        platform.console_messages,
    );
}
