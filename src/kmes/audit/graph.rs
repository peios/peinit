use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::service::{ServiceGraphFinding, ServiceGraphWarning};

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
