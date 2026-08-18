use crate::boundary::RegistryClient;
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{InitPlatform, InitRecoveryReason, InitRunError, InitRunResult, QuietLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoveryEnvironment {
    Ensure,
    Skip,
}

pub(super) fn enter_recovery<P>(
    platform: &mut P,
    reason: InitRecoveryReason,
    environment: RecoveryEnvironment,
) -> Result<InitRunResult, InitRunError>
where
    P: InitPlatform + ?Sized,
{
    // Never suppressed, at any quiet level and whoever owns the terminal: the
    // system is about to stop being the system, and the recovery shell is
    // taking that terminal next in any case.
    log_console_error(
        platform,
        &format!("peinit: entering recovery: {reason:?}\n"),
    );
    emit_recovery_audit_events(platform, &reason);
    if environment == RecoveryEnvironment::Ensure {
        let _ = platform.verify_root_writable();
        let _ = platform.mount_virtual_filesystems();
        let _ = platform.restore_random_seed();
        let _ = platform.ensure_machine_id();
        let _ = platform.set_clock_from_rtc();
        let mut supervisor = Supervisor::new(SupervisorSettings::default());
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
    if quiet.suppresses_status() {
        return;
    }
    let _ = platform.write_console_message(message);
}

/// Phase 1 output that survives a blackout: something went wrong, and silence
/// was a preference rather than an instruction to hide faults.
pub(super) fn log_console_error<P>(platform: &mut P, message: &str)
where
    P: InitPlatform + ?Sized,
{
    let _ = platform.write_console_message(message);
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
