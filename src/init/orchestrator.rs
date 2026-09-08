use crate::boot::BootMode;
use crate::boundary::{BoundaryError, Clock, RegistryClient};
use crate::supervisor::{Supervisor, SupervisorBootDispatch, SupervisorSettings};

use super::{
    InitConfig, InitPlatform, InitRecoveryReason, InitRunError, InitRunResult, InitRuntime,
    MachineIdStatus, QuietLevel,
};

mod recovery;
#[cfg(test)]
mod tests;

use crate::console_style::ConsoleTag;
use recovery::{
    RecoveryRegistryd, enter_recovery, log_console, log_console_error, log_console_raw,
    log_console_tagged, log_console_warn,
};

pub fn run_init<P, R, C, T>(
    config: InitConfig,
    platform: &mut P,
    registry: &mut R,
    clock: &mut C,
    runtime: &mut T,
) -> Result<InitRunResult, InitRunError>
where
    P: InitPlatform + ?Sized,
    R: RegistryClient,
    C: Clock + ?Sized,
    T: InitRuntime + ?Sized,
{
    platform.assert_pid1().map_err(InitRunError::Fatal)?;

    // Unconditional, and deliberately ahead of the command line: this line is
    // the only evidence peinit started at all, and it has to survive a peinit
    // that cannot read /proc/cmdline. `peios.quiet` cannot apply to it because
    // peinit has not seen the preference yet, which is the documented reason
    // the first lines of Phase 1 escape the policy.
    log_console(platform, QuietLevel::Verbose, "peinit: phase1 starting\n");

    // Then the command line, still before any Phase 1 work, because the
    // stage banner below needs the boot mode and the colour setting needs
    // `TERM`. That is safe this early for the same reason Phase 1's own mount
    // step is: /proc is `initramfs_provided`, prelude mount-moves it into the
    // root before exec'ing peinit, and `mount_virtual_filesystems` already
    // reads /proc/self/mountinfo before it mounts anything.
    //
    // The one behavioural consequence: a machine with BOTH an unreadable
    // command line and, say, missing privileges now reports the command line as
    // its recovery reason rather than the privileges. Both end in recovery, and
    // the command line is the more fundamental of the two.
    let command_line = match platform.read_kernel_command_line() {
        Ok(command_line) => command_line,
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::KernelCommandLine(error),
                RecoveryRegistryd::Start(Box::new(SupervisorSettings::new(config.phase2))),
            );
        }
    };
    crate::console_style::set_colour(!command_line.dumb_terminal);

    // The stage banner: punctuation between one PID 1 and the next. The
    // operator has just watched prelude hand over, and this says the real root
    // is in charge and in which mode.
    //
    // The mode named is the one this boot *starts* in. A later downgrade to
    // Safe -- from a dependency cycle involving a Critical service, say -- is
    // announced by its own message rather than by reprinting a banner, because
    // a second banner would read as a second stage.
    log_console_raw(
        platform,
        command_line.quiet,
        &crate::console_style::peinit_banner(if command_line.recovery {
            "RECOVERY MODE"
        } else if command_line.safe_mode || config.phase2.mode == BootMode::Safe {
            "Safe mode"
        } else {
            "Full boot"
        }),
    );


    // Before anything is attempted with them. SeCreateTokenPrivilege used to
    // surface only as an EPERM from kacs_create_token at the *first* service
    // start — registryd, in step 6 — so a peinit that could not mint tokens
    // entered recovery reporting what looked like a registryd problem, and
    // nothing named the privilege (PEI-365).
    if let Err(error) = platform.verify_privileges() {
        log_console_error(
            platform,
            &format!("peinit: required privileges are not held: {error:?}\n"),
        );
        return enter_recovery(
            platform,
            InitRecoveryReason::Privileges(error),
            RecoveryRegistryd::Start(Box::new(SupervisorSettings::new(config.phase2))),
        );
    }

    if let Err(error) = platform.verify_root_writable() {
        return enter_recovery(
            platform,
            InitRecoveryReason::RootWritable(error),
            RecoveryRegistryd::Start(Box::new(SupervisorSettings::new(config.phase2))),
        );
    }

    log_console(
        platform,
        QuietLevel::Verbose,
        "peinit: phase1 mounting virtual filesystems\n",
    );
    if let Err(error) = platform.mount_virtual_filesystems() {
        return enter_recovery(
            platform,
            InitRecoveryReason::VirtualFilesystems(error),
            RecoveryRegistryd::Start(Box::new(SupervisorSettings::new(config.phase2))),
        );
    }
    log_console_tagged(
        platform,
        QuietLevel::Verbose,
        ConsoleTag::Ok,
        "peinit: phase1 virtual filesystems mounted\n",
    );

    // The static /dev nodes everyone must be able to use. Advisory: a node
    // left unstamped is usable by administrators and denied to everyone else,
    // which is degraded, not unbootable.
    match platform.apply_device_node_policy() {
        Ok(report) => {
            for failure in &report.failures {
                log_console_warn(
                    platform,
                    &format!(
                        "peinit warning: device node {} ({}) descriptor failed: {}\n",
                        failure.path, failure.name, failure.message
                    ),
                );
            }
            for path in &report.missing {
                log_console(
                    platform,
                    QuietLevel::Verbose,
                    &format!("peinit: phase1 device node {path} absent; nothing to stamp\n"),
                );
            }
            if !report.applied.is_empty() {
                log_console_tagged(
                    platform,
                    QuietLevel::Verbose,
                    ConsoleTag::Ok,
                    &format!(
                        "peinit: phase1 device node policy applied to {} node(s)\n",
                        report.applied.len()
                    ),
                );
            }
        }
        Err(error) => log_console_warn(
            platform,
            &format!("peinit warning: device node policy failed: {error:?}\n"),
        ),
    }

    match platform.restore_random_seed() {
        Ok(true) => log_console_tagged(
            platform,
            QuietLevel::Verbose,
            ConsoleTag::Ok,
            "peinit: phase1 restored random seed\n",
        ),
        Ok(false) => {}
        Err(error) => log_console_warn(
            platform,
            &format!("peinit warning: random seed restore failed: {error:?}\n"),
        ),
    }

    match platform.ensure_machine_id() {
        Ok(MachineIdStatus::Existing) => {}
        Ok(MachineIdStatus::Generated) => log_console_tagged(
            platform,
            QuietLevel::Verbose,
            ConsoleTag::Ok,
            "peinit: phase1 generated machine-id\n",
        ),
        Ok(MachineIdStatus::ReplacedInvalid) => {
            log_console_warn(platform, "peinit warning: invalid machine-id replaced\n")
        }
        Ok(MachineIdStatus::Ephemeral { reason }) => log_console_error(
            platform,
            &format!(
                "peinit warning: machine-id not persisted ({reason}); \
                 using an identifier for this boot only\n"
            ),
        ),
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::MachineId(error),
                RecoveryRegistryd::Start(Box::new(SupervisorSettings::new(config.phase2))),
            );
        }
    }

    let quiet = command_line.quiet;
    // Settled here rather than after the boot-attempt checks, so that a
    // recovery entered from one of them starts its registryd with the settings
    // this boot asked for. The socket in particular has to be settled before
    // any registryd is launched, because binding it is the first thing a launch
    // does.
    let mut settings = SupervisorSettings::new(config.phase2);
    if command_line.safe_mode {
        settings.phase2.mode = BootMode::Safe;
    }

    if let Some(path) = &command_line.notify_socket_path {
        settings.notify_socket_path = path.clone();
    }
    // Carried on the supervisor because the runtime is entered with a
    // supervisor and nothing else — the same reason the notify socket is.
    settings.quiet = quiet;
    let counter = if command_line.recovery {
        None
    } else {
        match platform.read_boot_attempt_counter() {
            Ok(counter) => Some(counter),
            Err(error) => {
                return enter_recovery(
                    platform,
                    InitRecoveryReason::BootAttemptCounter(error),
                    RecoveryRegistryd::Start(Box::new(settings.clone())),
                );
            }
        }
    };
    let counter = if platform.increment_boot_attempt_counter().is_ok() {
        counter
    } else {
        Some(0)
    };

    if command_line.recovery {
        return enter_recovery(
            platform,
            InitRecoveryReason::ForcedByKernelCommandLine,
            RecoveryRegistryd::Start(Box::new(settings.clone())),
        );
    }
    let threshold = command_line
        .boot_attempt_threshold
        .unwrap_or(config.boot_attempt_threshold);
    // `peios.bootattempts=0` disables the check outright — the escape hatch for
    // a system whose recovery trigger is itself the problem, e.g. a root that
    // reports failure but boots fine.
    if threshold > 0
        && let Some(counter) = counter
        && counter >= threshold
    {
        return enter_recovery(
            platform,
            InitRecoveryReason::BootAttemptThresholdReached { counter, threshold },
            RecoveryRegistryd::Start(Box::new(settings.clone())),
        );
    }

    let mut supervisor = Supervisor::new(settings.clone());

    if let Err(error) = platform.set_clock_from_rtc() {
        return enter_recovery(
            platform,
            InitRecoveryReason::RtcClock(error),
            RecoveryRegistryd::Start(Box::new(settings.clone())),
        );
    }
    let registryd_started_at_ns = match clock.monotonic_ns() {
        Ok(now) => now,
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::Registryd(error),
                RecoveryRegistryd::Start(Box::new(settings.clone())),
            );
        }
    };
    log_console(platform, quiet, "peinit: phase1 starting registryd\n");
    if let Err(error) = platform.start_registryd(&mut supervisor, registry, registryd_started_at_ns)
    {
        return enter_recovery(
            platform,
            InitRecoveryReason::Registryd(error),
            RecoveryRegistryd::AlreadyAttempted,
        );
    }
    log_console_tagged(platform, quiet, ConsoleTag::Ok, "peinit: phase1 registryd started\n");
    // Phase 1.5: run the image's autorun scripts now that the registry is
    // serving, before Phase 2 enumerates Machine\System\Services — so a script
    // that seeds services (the seed-apply autorun) has them present when the boot
    // plan is built. peinit is a generic runner; the scripts own their effect and
    // lifecycle. Fail-open: the platform logs its own summary and never aborts
    // boot, so a missing dir or a failed script is a warning, not recovery.
    let _ = platform.run_autorun_scripts();
    let provisioning = match registry.read_provisioned_paths() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::Provisioning(error),
                RecoveryRegistryd::AlreadyAttempted,
            );
        }
    };
    for warning in &provisioning.warnings {
        log_console_error(
            platform,
            &format!(
                "peinit warning: provisioned path {} ignored: {}\n",
                warning.entry, warning.message
            ),
        );
    }
    let provisioning_report = match platform.provision_boot_paths(&provisioning.entries) {
        Ok(report) => report,
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::Provisioning(error),
                RecoveryRegistryd::AlreadyAttempted,
            );
        }
    };
    for warning in &provisioning_report.warnings {
        log_console_error(
            platform,
            &format!(
                "peinit warning: provisioned path {} at {} failed: {}\n",
                warning.entry, warning.path, warning.message
            ),
        );
    }
    if provisioning_report.has_required_failures() {
        for failure in &provisioning_report.required_failures {
            log_console_error(
                platform,
                &format!(
                    "peinit: required provisioned path {} at {} failed: {}\n",
                    failure.entry, failure.path, failure.message
                ),
            );
        }
        return enter_recovery(
            platform,
            InitRecoveryReason::Provisioning(BoundaryError::Recovery(format!(
                "{} required provisioned path(s) failed",
                provisioning_report.required_failures.len()
            ))),
            RecoveryRegistryd::AlreadyAttempted,
        );
    }
    let infrastructure = match platform.setup_infrastructure() {
        Ok(infrastructure) => infrastructure,
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::Infrastructure(error),
                RecoveryRegistryd::AlreadyAttempted,
            );
        }
    };
    for warning in infrastructure.warnings() {
        let _ = platform.log_phase1_warning(warning);
    }

    log_console(platform, quiet, "peinit: phase2 boot starting\n");
    let boot_dispatch = match supervisor.run_phase2_boot(registry, clock) {
        Ok(dispatch) => dispatch,
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::Phase2(error),
                RecoveryRegistryd::AlreadyAttempted,
            );
        }
    };
    log_phase2_boot_progress(platform, &boot_dispatch);
    emit_phase2_boot_audit_events(platform, &boot_dispatch);
    log_console_tagged(platform, quiet, ConsoleTag::Ok, "peinit: phase2 boot complete\n");

    match runtime.enter_runtime(supervisor, infrastructure) {
        Ok(()) => Ok(InitRunResult::RuntimeReturned),
        Err(error) => enter_recovery(
            platform,
            InitRecoveryReason::Runtime(error),
            RecoveryRegistryd::AlreadyAttempted,
        ),
    }
}

/// Emit one `graph.validation_error` KMES event per boot validation finding.
///
/// The console lines written by `log_phase2_boot_progress` are for whoever is
/// watching the boot; these are for whoever is reading the event stream
/// afterwards, which is the only account that survives the boot. The reload
/// path has emitted one event per finding since it existed; boot emitted
/// nothing at all, so validation problems were visible to an event consumer
/// only when they happened to arrive through a reload.
///
/// Every finding is emitted, not just the primary cause: the retained
/// `additional_reasons` are exactly the ones PSD-007 6.2 says must not be
/// suppressed, and dropping them here would put the retention back where it
/// started.
///
/// Failures to encode or emit are swallowed, matching the recovery path: an
/// audit sink that is not working must not be what stops a boot.
#[cfg(feature = "peios-boundary")]
fn emit_phase2_boot_audit_events<P>(platform: &mut P, dispatch: &SupervisorBootDispatch)
where
    P: InitPlatform + ?Sized,
{
    for downgrade in &dispatch.plan.safe_mode_downgrade {
        let Ok(event) = crate::kmes::encode_safe_mode_downgrade_event(downgrade) else {
            continue;
        };
        let _ = platform.emit_kmes_event(&event);
    }
    for blocked in &dispatch.plan.blocked {
        for reason in std::iter::once(&blocked.reason).chain(&blocked.additional_reasons) {
            let Ok(event) =
                crate::kmes::encode_boot_blocked_service_event(&blocked.service, reason)
            else {
                continue;
            };
            let _ = platform.emit_kmes_event(&event);
        }
    }
}

#[cfg(not(feature = "peios-boundary"))]
fn emit_phase2_boot_audit_events<P>(_platform: &mut P, _dispatch: &SupervisorBootDispatch)
where
    P: InitPlatform + ?Sized,
{
}

fn log_phase2_boot_progress<P>(platform: &mut P, dispatch: &SupervisorBootDispatch)
where
    P: InitPlatform + ?Sized,
{
    // Why the machine is in Safe mode, before anything about individual
    // services: it explains the shape of everything below it. The services
    // named here are deliberately not marked Failed, so this line and the
    // matching event are the only record that they were the cause.
    for downgrade in &dispatch.plan.safe_mode_downgrade {
        log_console_error(
            platform,
            &format!("peinit: boot downgraded to safe mode: {downgrade}\n"),
        );
    }
    // Configuration warnings next: they explain why the effective config is
    // not what the registry says, which is context for anything below.
    for warning in &dispatch.config_warnings {
        log_console_error(platform, &format!("peinit warning: {warning}\n"));
    }
    for blocked in &dispatch.plan.blocked {
        log_console_error(
            platform,
            &format!(
                "peinit: service {} failed: {:?}\n",
                blocked.service,
                blocked.reason.transition_cause()
            ),
        );
    }
}
