use peios::msgpack::Writer;

use crate::boot::phase2::{Phase2BootPlanError, Phase2BootRunError};
use crate::boundary::{BoundaryError, KmesEvent};
use crate::init::InitRecoveryReason;
use crate::supervisor::SupervisorError;

use super::graph::{encode_graph_cycle_error, encode_graph_service_error};
use crate::kmes::payload::{finish_event, write_str_field};

pub fn encode_init_recovery_events(
    reason: &InitRecoveryReason,
) -> Result<Vec<KmesEvent>, BoundaryError> {
    let mut events = vec![encode_recovery_entry_event(reason)?];
    if let InitRecoveryReason::Phase2(error) = reason {
        push_phase2_graph_error_events(error, &mut events)?;
    }
    Ok(events)
}

fn encode_recovery_entry_event(reason: &InitRecoveryReason) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(2);
    write_str_field(&mut writer, "reason", recovery_reason_label(reason));
    write_str_field(&mut writer, "detail", &format!("{reason:?}"));
    finish_event("recovery.entered", writer)
}

fn push_phase2_graph_error_events(
    error: &SupervisorError,
    out: &mut Vec<KmesEvent>,
) -> Result<(), BoundaryError> {
    let SupervisorError::Phase2Boot(Phase2BootRunError::Plan(plan_error)) = error else {
        return Ok(());
    };
    if let Some(event) = encode_phase2_plan_graph_error_event(plan_error)? {
        out.push(event);
    }
    Ok(())
}

fn encode_phase2_plan_graph_error_event(
    error: &Phase2BootPlanError,
) -> Result<Option<KmesEvent>, BoundaryError> {
    let event = match error {
        Phase2BootPlanError::DuplicateService { service } => Some(encode_graph_service_error(
            "phase2_boot",
            "duplicate_service",
            service,
        )?),
        Phase2BootPlanError::MissingServiceDefinition { service } => Some(
            encode_graph_service_error("phase2_boot", "missing_service_definition", service)?,
        ),
        Phase2BootPlanError::Cycle { services } => {
            Some(encode_graph_cycle_error("phase2_boot", services)?)
        }
        Phase2BootPlanError::InvalidMaxParallelStarts
        | Phase2BootPlanError::OperationIdAllocation(_)
        | Phase2BootPlanError::JobIdAllocation(_) => None,
    };
    Ok(event)
}

fn recovery_reason_label(reason: &InitRecoveryReason) -> &'static str {
    match reason {
        InitRecoveryReason::KernelCommandLine(_) => "kernel_command_line",
        InitRecoveryReason::BootAttemptCounter(_) => "boot_attempt_counter",
        InitRecoveryReason::ForcedByKernelCommandLine => "forced_by_kernel_command_line",
        InitRecoveryReason::BootAttemptThresholdReached { .. } => "boot_attempt_threshold_reached",
        InitRecoveryReason::RootWritable(_) => "root_writable",
        InitRecoveryReason::VirtualFilesystems(_) => "virtual_filesystems",
        InitRecoveryReason::MachineId(_) => "machine_id",
        InitRecoveryReason::RtcClock(_) => "rtc_clock",
        InitRecoveryReason::Registryd(_) => "registryd",
        InitRecoveryReason::Provisioning(_) => "provisioning",
        InitRecoveryReason::Infrastructure(_) => "infrastructure",
        InitRecoveryReason::Phase2(_) => "phase2",
        InitRecoveryReason::Runtime(_) => "runtime",
    }
}
