use peios::msgpack::Writer;

use crate::boot::phase2::{BlockedReason, SafeModeDowngrade};
use crate::boundary::{BoundaryError, KmesEvent};
use crate::service::{ServiceDependencyKind, ServiceGraphFinding, ServiceGraphWarning};

use crate::kmes::labels::service_dependency_kind_label;
use crate::kmes::payload::{
    finish_event, write_str_field, write_string_array_field, write_uint_field,
};

pub fn encode_graph_validation_warning_event(
    phase: &str,
    warning: &ServiceGraphWarning,
) -> Result<KmesEvent, BoundaryError> {
    match warning {
        ServiceGraphWarning::AliveReadinessWithHardDependents {
            service,
            dependents,
        } => {
            let mut writer = Writer::new();
            writer.write_map(5);
            write_str_field(&mut writer, "phase", phase);
            write_str_field(
                &mut writer,
                "warning",
                "alive_readiness_with_hard_dependents",
            );
            write_str_field(&mut writer, "service", service);
            write_string_array_field(&mut writer, "dependents", dependents);
            write_str_field(
                &mut writer,
                "message",
                &format!(
                    "service {service} uses Alive readiness while hard dependents require readiness"
                ),
            );
            finish_event("graph.validation_warning", writer)
        }
        ServiceGraphWarning::UnfilledRole { role, services } => {
            let mut writer = Writer::new();
            writer.write_map(5);
            write_str_field(&mut writer, "phase", phase);
            write_str_field(&mut writer, "warning", "unfilled_role");
            write_str_field(&mut writer, "role", role);
            write_string_array_field(&mut writer, "services", services);
            write_str_field(
                &mut writer,
                "message",
                &format!("no service provides {role}, which other services need to start"),
            );
            finish_event("graph.validation_warning", writer)
        }
    }
}

pub fn encode_graph_validation_error_event(
    phase: &str,
    finding: &ServiceGraphFinding,
) -> Result<KmesEvent, BoundaryError> {
    match finding {
        ServiceGraphFinding::InvalidServiceName { service } => {
            encode_graph_service_error(phase, "invalid_service_name", service)
        }
        ServiceGraphFinding::DuplicateService { service } => {
            encode_graph_service_error(phase, "duplicate_service", service)
        }
        ServiceGraphFinding::MissingHardDependency {
            service,
            target,
            kind,
        } => {
            let mut writer = Writer::new();
            writer.write_map(6);
            write_str_field(&mut writer, "phase", phase);
            write_str_field(&mut writer, "finding", "missing_hard_dependency");
            write_str_field(&mut writer, "service", service);
            write_str_field(&mut writer, "target", target);
            write_str_field(
                &mut writer,
                "dependency_kind",
                service_dependency_kind_label(*kind),
            );
            write_str_field(
                &mut writer,
                "message",
                &format!("service {service} has missing hard dependency {target}"),
            );
            finish_event("graph.validation_error", writer)
        }
        ServiceGraphFinding::Cycle { services } => encode_graph_cycle_error(phase, services),
        ServiceGraphFinding::ConflictingBootServices { service, target } => {
            let mut writer = Writer::new();
            writer.write_map(5);
            write_str_field(&mut writer, "phase", phase);
            write_str_field(&mut writer, "finding", "conflicting_boot_services");
            write_str_field(&mut writer, "service", service);
            write_str_field(&mut writer, "target", target);
            write_str_field(
                &mut writer,
                "message",
                &format!("boot-triggered services {service} and {target} conflict"),
            );
            finish_event("graph.validation_error", writer)
        }
        ServiceGraphFinding::InvalidHealthCheckRestartWindow {
            service,
            retries,
            interval_secs,
            restart_window_secs,
        } => {
            let mut writer = Writer::new();
            writer.write_map(7);
            write_str_field(&mut writer, "phase", phase);
            write_str_field(
                &mut writer,
                "finding",
                "invalid_health_check_restart_window",
            );
            write_str_field(&mut writer, "service", service);
            write_uint_field(&mut writer, "retries", u64::from(*retries));
            write_uint_field(&mut writer, "interval_secs", *interval_secs);
            write_uint_field(&mut writer, "restart_window_secs", *restart_window_secs);
            write_str_field(
                &mut writer,
                "message",
                &format!("service {service} has invalid health-check restart-window timing"),
            );
            finish_event("graph.validation_error", writer)
        }
        ServiceGraphFinding::UnschedulableHealthCheck {
            service,
            service_type,
        } => {
            let mut writer = Writer::new();
            writer.write_map(5);
            write_str_field(&mut writer, "phase", phase);
            write_str_field(&mut writer, "finding", "unschedulable_health_check");
            write_str_field(&mut writer, "service", service);
            write_str_field(
                &mut writer,
                "service_type",
                super::super::labels::service_type_label(*service_type),
            );
            write_str_field(
                &mut writer,
                "message",
                &format!(
                    "service {service} declares a HealthCheck, which is scheduled \
                     for Simple services only"
                ),
            );
            finish_event("graph.validation_error", writer)
        }
        ServiceGraphFinding::InvalidTimerSchedule {
            service,
            schedule,
            message,
        } => {
            let mut writer = Writer::new();
            writer.write_map(6);
            write_str_field(&mut writer, "phase", phase);
            write_str_field(&mut writer, "finding", "invalid_timer_schedule");
            write_str_field(&mut writer, "service", service);
            write_str_field(&mut writer, "schedule", schedule);
            write_str_field(&mut writer, "parse_error", message);
            write_str_field(
                &mut writer,
                "message",
                &format!("service {service} has invalid timer schedule"),
            );
            finish_event("graph.validation_error", writer)
        }
    }
}

pub(super) fn encode_graph_service_error(
    phase: &str,
    finding: &'static str,
    service: &str,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(4);
    write_str_field(&mut writer, "phase", phase);
    write_str_field(&mut writer, "finding", finding);
    write_str_field(&mut writer, "service", service);
    write_str_field(
        &mut writer,
        "message",
        &format!("service {service} graph validation finding: {finding}"),
    );
    finish_event("graph.validation_error", writer)
}

pub(super) fn encode_graph_cycle_error(
    phase: &str,
    services: &[String],
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(4);
    write_str_field(&mut writer, "phase", phase);
    write_str_field(&mut writer, "finding", "cycle");
    write_string_array_field(&mut writer, "services", services);
    write_str_field(
        &mut writer,
        "message",
        &format!("dependency cycle: {}", services.join(" -> ")),
    );
    finish_event("graph.validation_error", writer)
}

/// Encode one boot-time blocked-service finding as a `graph.validation_error`.
///
/// Deliberately the same event type as the reload path's findings, separated
/// by the `phase` field ("boot" here, "reload_config" there). A consumer
/// filtering for validation problems then gets both without knowing two type
/// names, and the phase tells it which regime it is looking at — boot marks
/// individual services and continues, reload rejects the whole reload.
///
/// `BlockedReason` is not `ServiceGraphFinding` and does not map onto it one
/// for one: `HardDependencyBlocked` — blocked *because a dependency is
/// blocked* — is real information the reload path cannot produce, since reload
/// rejects wholesale rather than propagating a block. It gets its own
/// `finding` value rather than being flattened into the missing-dependency
/// case, which would claim the target does not exist when it does.
pub fn encode_boot_blocked_service_event(
    service: &str,
    reason: &BlockedReason,
) -> Result<KmesEvent, BoundaryError> {
    encode_blocked_service_event("boot", service, reason)
}

/// A key a reload found but could not decode: the same `validation_error`
/// finding the boot emits for one, under the `reload_config` phase, so a
/// consumer sees the service fail the same way whichever path read it
/// (PEI-621).
pub fn encode_reload_undecodable_service_event(
    service: &crate::boundary::UndecodableService,
) -> Result<KmesEvent, BoundaryError> {
    encode_blocked_service_event(
        "reload_config",
        &service.name,
        &BlockedReason::ValidationError {
            message: crate::service::undecodable_message(service),
        },
    )
}

fn encode_blocked_service_event(
    phase: &str,
    service: &str,
    reason: &BlockedReason,
) -> Result<KmesEvent, BoundaryError> {
    match reason {
        BlockedReason::CycleDetected { services } => encode_graph_cycle_error(phase, services),
        BlockedReason::HardDependencyUnavailable { target, kind } => encode_boot_dependency_error(
            phase,
            "missing_hard_dependency",
            service,
            target,
            *kind,
            &format!("service {service} has missing hard dependency {target}"),
        ),
        BlockedReason::HardDependencyBlocked { target, kind } => encode_boot_dependency_error(
            phase,
            "hard_dependency_blocked",
            service,
            target,
            *kind,
            &format!("service {service} is blocked because hard dependency {target} is blocked"),
        ),
        BlockedReason::ConflictingBootService { target } => {
            let mut writer = Writer::new();
            writer.write_map(5);
            write_str_field(&mut writer, "phase", phase);
            write_str_field(&mut writer, "finding", "conflicting_boot_services");
            write_str_field(&mut writer, "service", service);
            write_str_field(&mut writer, "target", target);
            write_str_field(
                &mut writer,
                "message",
                &format!("boot-triggered services {service} and {target} conflict"),
            );
            finish_event("graph.validation_error", writer)
        }
        BlockedReason::ValidationError { message } => {
            let mut writer = Writer::new();
            writer.write_map(5);
            write_str_field(&mut writer, "phase", phase);
            write_str_field(&mut writer, "finding", "validation_error");
            write_str_field(&mut writer, "service", service);
            write_str_field(&mut writer, "detail", message);
            write_str_field(
                &mut writer,
                "message",
                &format!("service {service} failed validation: {message}"),
            );
            finish_event("graph.validation_error", writer)
        }
    }
}

fn encode_boot_dependency_error(
    phase: &str,
    finding: &'static str,
    service: &str,
    target: &str,
    kind: ServiceDependencyKind,
    message: &str,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(6);
    write_str_field(&mut writer, "phase", phase);
    write_str_field(&mut writer, "finding", finding);
    write_str_field(&mut writer, "service", service);
    write_str_field(&mut writer, "target", target);
    write_str_field(
        &mut writer,
        "dependency_kind",
        service_dependency_kind_label(kind),
    );
    write_str_field(&mut writer, "message", message);
    finish_event("graph.validation_error", writer)
}

/// Encode why a Full boot was downgraded to Safe mode.
///
/// A distinct event type from `graph.validation_error`, because it is a
/// different claim: not "this service is broken" but "the machine is running a
/// reduced service set, and here is what forced that". The services named are
/// deliberately not marked Failed — Safe mode was never going to start them —
/// so this event is the only record that they were the cause.
pub fn encode_safe_mode_downgrade_event(
    downgrade: &SafeModeDowngrade,
) -> Result<KmesEvent, BoundaryError> {
    match downgrade {
        SafeModeDowngrade::CriticalCycle { services } => {
            let mut writer = Writer::new();
            writer.write_map(3);
            write_str_field(&mut writer, "finding", "critical_cycle");
            write_string_array_field(&mut writer, "services", services);
            write_str_field(
                &mut writer,
                "message",
                &format!(
                    "boot downgraded to safe mode: critical service in dependency cycle {}",
                    services.join(" -> "),
                ),
            );
            finish_event("boot.safe_mode_downgrade", writer)
        }
        SafeModeDowngrade::CriticalBootConflict { service, target } => {
            let mut writer = Writer::new();
            writer.write_map(4);
            write_str_field(&mut writer, "finding", "critical_boot_conflict");
            write_str_field(&mut writer, "service", service);
            write_str_field(&mut writer, "target", target);
            write_str_field(
                &mut writer,
                "message",
                &format!(
                    "boot downgraded to safe mode: critical boot-triggered services {service} and {target} conflict",
                ),
            );
            finish_event("boot.safe_mode_downgrade", writer)
        }
    }
}
