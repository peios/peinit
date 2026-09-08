use crate::boundary::RegistryClient;
use crate::console_style::ConsoleTag;
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{InitPlatform, InitRecoveryReason, InitRunError, InitRunResult, QuietLevel};

/// Whether recovery must still attempt registryd.
///
/// Recovery attempts one **exactly once, and only if Phase 1 has not already
/// tried**. Both halves of that are load-bearing:
///
///   - Skipping it entirely — which is what the Phase 1 failure paths used to
///     do — hands the operator a shell with no configuration store, so every
///     `reg`-family tool fails. An RTC failure was the starkest case: nothing
///     about registryd had gone wrong and the operator still got no registry.
///   - Running a second one is worse. On success the first is serving and its
///     activation has been retained for the runtime; on failure a process may
///     still have forked before the failure was reported. Either way a second
///     daemon binds over the first's notify socket — `NotifySocket::bind`
///     unlinks the path first, so it succeeds rather than reporting
///     `EADDRINUSE` — and opens the same loregd hive files behind its back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RecoveryRegistryd {
    /// Phase 1 has not reached its registryd start. Recovery attempts one with
    /// the settings Phase 1 would have used, and ignores any failure: a shell
    /// is delivered regardless of registryd's state.
    Start(Box<SupervisorSettings>),
    /// Phase 1 already ran its registryd start. Recovery leaves it alone,
    /// whether it succeeded or not.
    AlreadyAttempted,
}

/// Enter recovery mode: complete whatever of Phase 1 has not run, get a
/// registryd up if nothing has tried yet, and hand the operator a shell.
///
/// The Phase 1 catch-up is unconditional. The steps are individually
/// idempotent, so "complete steps 1-5 if not already done" is served by simply
/// running them and ignoring the failures. It used to be conditional, and the
/// condition was wrong in the direction that mattered: a `/dev/shm` or
/// `/sys/fs/cgroup` mount failure delivered a shell with the random seed
/// unrestored, no machine ID, and **the clock unset**, so every timestamp in
/// the session was wrong — in exactly the situations the mode exists for.
pub(super) fn enter_recovery<P>(
    platform: &mut P,
    reason: InitRecoveryReason,
    registryd: RecoveryRegistryd,
) -> Result<InitRunResult, InitRunError>
where
    P: InitPlatform + ?Sized,
{
    // Never suppressed, at any quiet level and whoever owns the terminal: the
    // system is about to stop being the system, and the recovery shell is
    // taking that terminal next in any case.
    log_console_critical(
        platform,
        &format!("peinit: entering recovery: {reason:?}\n"),
    );
    emit_recovery_audit_events(platform, &reason);
    let _ = platform.verify_root_writable();
    let _ = platform.mount_virtual_filesystems();
    let _ = platform.restore_random_seed();
    let _ = platform.ensure_machine_id();
    let _ = platform.set_clock_from_rtc();
    if let RecoveryRegistryd::Start(settings) = registryd {
        // The settings Phase 1 parsed, not defaults. A machine booted with
        // `peios.notifysocket=` set would otherwise get a recovery registryd
        // pointed at the default path, and `peios.quiet` would not apply.
        let mut supervisor = Supervisor::new(*settings);
        let mut registry = NoRecoveryRegistry;
        let _ = platform.start_registryd(&mut supervisor, &mut registry, 0);
    }
    platform
        .run_recovery_forever(&reason)
        .map_err(|error| InitRunError::RecoveryFailed {
            reason: Box::new(reason.clone()),
            error,
        })?;
    Ok(InitRunResult::RecoveryReturned { reason })
}

/// Phase 1 progress, subject to `peios.quiet`.
///
/// Only the blackout half of the policy can apply here: Phase 1 runs before any
/// service exists, so there is no terminal for anything else to own. The
/// ownership rule starts mattering when the runtime does.
pub(super) fn log_console<P>(platform: &mut P, quiet: QuietLevel, message: &str)
where
    P: InitPlatform + ?Sized,
{
    log_console_tagged(platform, quiet, ConsoleTag::None, message);
}

/// Phase-1 progress that reports an outcome, so it earns a tag.
///
/// Phase 1 reaches the console by a different path than the runtime does —
/// raw strings through `InitPlatform::write_console_message`, rather than a
/// `ConsoleMessage` through the `ConsoleSink` — so the rendering has to happen
/// here too. Both call the same renderer, which is the point of keeping it in
/// `console_style` rather than in either path.
pub(super) fn log_console_tagged<P>(
    platform: &mut P,
    quiet: QuietLevel,
    tag: ConsoleTag,
    message: &str,
) where
    P: InitPlatform + ?Sized,
{
    if quiet.suppresses_status() {
        return;
    }
    let _ = platform.write_console_message(tag, message);
}

/// Phase-1 output with nothing to report but the fact of it: a banner, or a
/// line that is already its own punctuation. Never tagged, never padded.
pub(super) fn log_console_raw<P>(platform: &mut P, quiet: QuietLevel, message: &str)
where
    P: InitPlatform + ?Sized,
{
    if quiet.suppresses_status() {
        return;
    }
    let _ = platform.write_console_message(ConsoleTag::Bare, message);
}

/// Phase 1 output that survives a blackout: something went wrong, and silence
/// was a preference rather than an instruction to hide faults.
pub(super) fn log_console_error<P>(platform: &mut P, message: &str)
where
    P: InitPlatform + ?Sized,
{
    let _ = platform.write_console_message(ConsoleTag::Failed, message);
}

/// Phase-1 output for something wrong that the boot survives.
pub(super) fn log_console_warn<P>(platform: &mut P, message: &str)
where
    P: InitPlatform + ?Sized,
{
    let _ = platform.write_console_message(ConsoleTag::Warn, message);
}

/// Phase-1 output for losing the machine.
pub(super) fn log_console_critical<P>(platform: &mut P, message: &str)
where
    P: InitPlatform + ?Sized,
{
    let _ = platform.write_console_message(ConsoleTag::Crit, message);
}

#[cfg(feature = "peios-boundary")]
fn emit_recovery_audit_events<P>(platform: &mut P, reason: &InitRecoveryReason)
where
    P: InitPlatform + ?Sized,
{
    let Ok(events) = crate::kmes::encode_init_recovery_events(reason) else {
        return;
    };
    for event in events {
        let _ = platform.emit_kmes_event(&event);
    }
}

#[cfg(not(feature = "peios-boundary"))]
fn emit_recovery_audit_events<P>(_platform: &mut P, _reason: &InitRecoveryReason)
where
    P: InitPlatform + ?Sized,
{
}

#[derive(Debug)]
struct NoRecoveryRegistry;

impl RegistryClient for NoRecoveryRegistry {
    fn read_service_definitions(
        &mut self,
    ) -> Result<Vec<crate::service::ServiceDefinition>, crate::boundary::BoundaryError> {
        // Recovery never boots configured services; it only needs registryd up.
        Ok(Vec::new())
    }

    fn provision_base_registry(&mut self) -> Result<(), crate::boundary::BoundaryError> {
        // Still create the base structure (Machine\System\{Services,Init} +
        // SchemaVersion) so the recovery environment matches a normal boot — a
        // fresh system's recovery shell should see, and be able to build on, the
        // same registry layout. Idempotent, so a provisioned system is a no-op.
        // Gated like the rest of the LCS path: LcsRegistryClient only exists with
        // the peios-registry feature (the real Linux build); without it there is
        // no registry to provision.
        #[cfg(feature = "peios-registry")]
        {
            crate::registry::LcsRegistryClient.provision_base_registry()
        }
        #[cfg(not(feature = "peios-registry"))]
        {
            Ok(())
        }
    }
}
