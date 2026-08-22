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

use recovery::{RecoveryEnvironment, enter_recovery, log_console, log_console_error};

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
    // Before the command line is read, so `peios.quiet` cannot apply yet:
    // peinit cannot honour a preference it has not seen. These few lines are
    // also the only evidence peinit started at all, which makes them the right
    // ones to be unconditional.
    log_console(platform, QuietLevel::Verbose, "peinit: phase1 starting\n");

    if let Err(error) = platform.verify_root_writable() {
        return enter_recovery(
            platform,
            InitRecoveryReason::RootWritable(error),
            RecoveryEnvironment::Skip,
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
            RecoveryEnvironment::Skip,
        );
    }
    log_console(
        platform,
        QuietLevel::Verbose,
        "peinit: phase1 virtual filesystems mounted\n",
    );

    // The static /dev nodes everyone must be able to use. Advisory: a node
    // left unstamped is usable by administrators and denied to everyone else,
    // which is degraded, not unbootable.
    match platform.apply_device_node_policy() {
        Ok(report) => {
            for failure in &report.failures {
                log_console_error(
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
                log_console(
                    platform,
                    QuietLevel::Verbose,
                    &format!(
                        "peinit: phase1 device node policy applied to {} node(s)\n",
                        report.applied.len()
                    ),
                );
            }
        }
        Err(error) => log_console_error(
            platform,
            &format!("peinit warning: device node policy failed: {error:?}\n"),
        ),
    }

    match platform.restore_random_seed() {
        Ok(true) => log_console(
            platform,
            QuietLevel::Verbose,
            "peinit: phase1 restored random seed\n",
        ),
        Ok(false) => {}
        Err(error) => log_console_error(
            platform,
            &format!("peinit warning: random seed restore failed: {error:?}\n"),
        ),
    }

    match platform.ensure_machine_id() {
        Ok(MachineIdStatus::Existing) => {}
        Ok(MachineIdStatus::Generated) => log_console(
            platform,
            QuietLevel::Verbose,
            "peinit: phase1 generated machine-id\n",
        ),
        Ok(MachineIdStatus::ReplacedInvalid) => {
            log_console_error(platform, "peinit warning: invalid machine-id replaced\n")
        }
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::MachineId(error),
                RecoveryEnvironment::Ensure,
            );
        }
    }

    let command_line = match platform.read_kernel_command_line() {
        Ok(command_line) => command_line,
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::KernelCommandLine(error),
                RecoveryEnvironment::Ensure,
            );
        }
    };
    let quiet = command_line.quiet;
    let counter = if command_line.recovery {
        None
    } else {
        match platform.read_boot_attempt_counter() {
            Ok(counter) => Some(counter),
            Err(error) => {
                return enter_recovery(
                    platform,
                    InitRecoveryReason::BootAttemptCounter(error),
                    RecoveryEnvironment::Ensure,
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
            RecoveryEnvironment::Ensure,
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
            RecoveryEnvironment::Ensure,
        );
    }

    let mut settings = SupervisorSettings::new(config.phase2);
    if command_line.safe_mode {
        settings.phase2.mode = BootMode::Safe;
    }
    // Phase-1 command-line overrides. The socket has to be settled before
    // registryd is launched below, because binding it is the first thing that
    // launch does.
    if let Some(path) = &command_line.notify_socket_path {
        settings.notify_socket_path = path.clone();
    }
    // Carried on the supervisor because the runtime is entered with a
    // supervisor and nothing else — the same reason the notify socket is.
    settings.quiet = quiet;
    let mut supervisor = Supervisor::new(settings);

    if let Err(error) = platform.set_clock_from_rtc() {
        return enter_recovery(
            platform,
            InitRecoveryReason::RtcClock(error),
            RecoveryEnvironment::Skip,
        );
    }
    let registryd_started_at_ns = match clock.monotonic_ns() {
        Ok(now) => now,
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::Registryd(error),
                RecoveryEnvironment::Skip,
            );
        }
    };
    log_console(platform, quiet, "peinit: phase1 starting registryd\n");
    if let Err(error) = platform.start_registryd(&mut supervisor, registry, registryd_started_at_ns)
    {
        return enter_recovery(
            platform,
            InitRecoveryReason::Registryd(error),
            RecoveryEnvironment::Skip,
        );
    }
    log_console(platform, quiet, "peinit: phase1 registryd started\n");
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
                RecoveryEnvironment::Ensure,
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
                RecoveryEnvironment::Ensure,
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
            RecoveryEnvironment::Ensure,
        );
    }
    let infrastructure = match platform.setup_infrastructure() {
        Ok(infrastructure) => infrastructure,
        Err(error) => {
            return enter_recovery(
                platform,
                InitRecoveryReason::Infrastructure(error),
                RecoveryEnvironment::Skip,
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
                RecoveryEnvironment::Ensure,
            );
        }
    };
    log_phase2_boot_progress(platform, &boot_dispatch);
    log_console(platform, quiet, "peinit: phase2 boot complete\n");

    match runtime.enter_runtime(supervisor, infrastructure) {
        Ok(()) => Ok(InitRunResult::RuntimeReturned),
        Err(error) => enter_recovery(
            platform,
            InitRecoveryReason::Runtime(error),
            RecoveryEnvironment::Ensure,
        ),
    }
}

fn log_phase2_boot_progress<P>(platform: &mut P, dispatch: &SupervisorBootDispatch)
where
    P: InitPlatform + ?Sized,
{
    // Configuration warnings first: they explain why the effective config is
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
