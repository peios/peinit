use crate::boot::phase2::{Phase2BootPlanError, Phase2BootRunError};
use crate::boundary::{BoundaryError, KmesEvent};
use crate::init::InitRecoveryReason;
use crate::supervisor::SupervisorError;

use super::graph::{GraphPhase, encode_graph_cycle_error, encode_graph_service_error};
use crate::kmes::EventCollector;
use crate::kmes::payload::Payload;
use crate::kmes::types::{GRAPH_VALIDATION_FAILED, RECOVERY_ENTERED};

/// `peinit.recovery.entered`, and the phase-2 plan's own finding when the
/// plan is what failed. The first is essential; the finding is a
/// `peinit.graph.validation.failed`, which the collector's policy governs.
pub fn collect_init_recovery_events(
    reason: &InitRecoveryReason,
    out: &mut EventCollector<'_>,
) -> Result<(), BoundaryError> {
    out.push(RECOVERY_ENTERED, |_| encode_recovery_entry_event(reason))?;
    if let InitRecoveryReason::Phase2(SupervisorError::Phase2Boot(Phase2BootRunError::Plan(
        plan_error,
    ))) = reason
        && phase2_plan_finding(plan_error)
    {
        out.push(GRAPH_VALIDATION_FAILED, |_| {
            encode_phase2_plan_graph_error_event(plan_error)
        })?;
    }
    Ok(())
}

fn encode_recovery_entry_event(reason: &InitRecoveryReason) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("outcome.reason", recovery_reason_label(reason));
    payload.set_opt("outcome.detail", recovery_detail(reason));
    payload.finish(RECOVERY_ENTERED)
}

/// What went wrong, in the error's own words where it has some. Rust Debug
/// output is never written into an event (PGSS §6.5), so a reason with no
/// words of its own has no detail.
fn recovery_detail(reason: &InitRecoveryReason) -> Option<String> {
    match reason {
        InitRecoveryReason::KernelCommandLine(error)
        | InitRecoveryReason::Privileges(error)
        | InitRecoveryReason::BootAttemptCounter(error)
        | InitRecoveryReason::RootWritable(error)
        | InitRecoveryReason::VirtualFilesystems(error)
        | InitRecoveryReason::MachineId(error)
        | InitRecoveryReason::RtcClock(error)
        | InitRecoveryReason::Registryd(error)
        | InitRecoveryReason::Provisioning(error)
        | InitRecoveryReason::Infrastructure(error)
        | InitRecoveryReason::Runtime(error) => error.text().map(str::to_string),
        InitRecoveryReason::BootAttemptThresholdReached { counter, threshold } => Some(format!(
            "the boot attempt counter is {counter}, at or past the threshold of {threshold}"
        )),
        InitRecoveryReason::ForcedByKernelCommandLine | InitRecoveryReason::Phase2(_) => None,
    }
}

/// Whether a plan error is a finding about the service graph, rather than
/// peinit failing to build a plan for its own reasons.
fn phase2_plan_finding(error: &Phase2BootPlanError) -> bool {
    matches!(
        error,
        Phase2BootPlanError::DuplicateService { .. }
            | Phase2BootPlanError::MissingServiceDefinition { .. }
            | Phase2BootPlanError::Cycle { .. }
    )
}

fn encode_phase2_plan_graph_error_event(
    error: &Phase2BootPlanError,
) -> Result<KmesEvent, BoundaryError> {
    match error {
        Phase2BootPlanError::DuplicateService { service } => {
            encode_graph_service_error(GraphPhase::Phase2Boot, "duplicate-service", service)
        }
        Phase2BootPlanError::MissingServiceDefinition { service } => encode_graph_service_error(
            GraphPhase::Phase2Boot,
            "missing-service-definition",
            service,
        ),
        Phase2BootPlanError::Cycle { services } => {
            encode_graph_cycle_error(GraphPhase::Phase2Boot, services)
        }
        Phase2BootPlanError::InvalidMaxParallelStarts
        | Phase2BootPlanError::OperationIdAllocation(_)
        | Phase2BootPlanError::JobIdAllocation(_) => Err(BoundaryError::Kmes(
            "not a finding about the service graph".to_string(),
        )),
    }
}

fn recovery_reason_label(reason: &InitRecoveryReason) -> &'static str {
    match reason {
        InitRecoveryReason::KernelCommandLine(_) => "kernel-command-line",
        InitRecoveryReason::Privileges(_) => "privileges",
        InitRecoveryReason::BootAttemptCounter(_) => "boot-attempt-counter",
        InitRecoveryReason::ForcedByKernelCommandLine => "forced-by-kernel-command-line",
        InitRecoveryReason::BootAttemptThresholdReached { .. } => "boot-attempt-threshold-reached",
        InitRecoveryReason::RootWritable(_) => "root-writable",
        InitRecoveryReason::VirtualFilesystems(_) => "virtual-filesystems",
        InitRecoveryReason::MachineId(_) => "machine-id",
        InitRecoveryReason::RtcClock(_) => "rtc-clock",
        InitRecoveryReason::Registryd(_) => "registryd",
        InitRecoveryReason::Provisioning(_) => "provisioning",
        InitRecoveryReason::Infrastructure(_) => "infrastructure",
        InitRecoveryReason::Phase2(_) => "phase2",
        InitRecoveryReason::Runtime(_) => "runtime",
    }
}
