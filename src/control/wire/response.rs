use serde_json::json;

use crate::control::query::{
    OperationStatusView, ServiceListItem, ServiceStatusView, ServiceStatusWarning,
    ServiceStatusWarningType,
};
use crate::control::reload_config::ReloadConfigOutcome;
use crate::ids::OperationId;
use crate::service::runtime::{ServiceState, TransitionCause};

use super::model::{ControlErrorCode, ControlResponseStatus};
use labels::{
    job_type_wire, operation_source_wire, operation_state_wire, operation_type_wire,
    service_health_wire, service_state_wire, transition_cause_wire,
};
pub use time::ControlResponseTimeProjection;

mod labels;
mod time;

pub fn control_client_error_message(error: ControlErrorCode) -> &'static str {
    match error {
        ControlErrorCode::MalformedRequest => "malformed control request",
        ControlErrorCode::RequestTooLarge => "control request too large",
        ControlErrorCode::InvalidCommand => "invalid control command",
        ControlErrorCode::InvalidArguments => "invalid control arguments",
        _ => "control request failed",
    }
}

pub fn control_system_ok_response_line() -> Result<Vec<u8>, serde_json::Error> {
    let response = json!({
        "status": ControlResponseStatus::Ok.as_str(),
    });
    response_line(&response)
}

pub fn control_status_response_line(
    view: &ServiceStatusView,
    time: ControlResponseTimeProjection,
) -> Result<Vec<u8>, serde_json::Error> {
    let started_at_ns = view.current_job.as_ref().and_then(|job| job.started_at_ns);
    let response = json!({
        "status": ControlResponseStatus::Ok.as_str(),
        "service": view.service.as_str(),
        "display_name": view.display_name.as_deref(),
        "description": view.description.as_deref(),
        "state": service_state_wire(view.state),
        "cause": view.cause.map(transition_cause_wire),
        "status_text": view.status_text.as_deref(),
        "current_job": view.current_job.as_ref().map(|job| {
            json!({
                "id": job.id.to_canonical_string(),
                "type": job_type_wire(job.job_type),
                "pid": job.pid,
                "started_at": job.started_at_ns.map(|started_at_ns| time.realtime_timestamp(started_at_ns)),
                "identity": job.identity.as_str(),
            })
        }),
        "current_operation": view.current_operation.as_ref().map(|operation| {
            json!({
                "id": operation.id.to_canonical_string(),
                "type": operation_type_wire(operation.operation_type),
                "source": operation_source_wire(operation.source),
            })
        }),
        "health": view.health.map(service_health_wire),
        "uptime_seconds": time.uptime_seconds(started_at_ns),
        "definition_removed": view.definition_removed,
        "warnings": view.warnings.iter().map(|warning| status_warning_json(warning, time)).collect::<Vec<_>>(),
    });
    response_line(&response)
}

fn status_warning_json(
    warning: &ServiceStatusWarning,
    time: ControlResponseTimeProjection,
) -> serde_json::Value {
    json!({
        "path": warning.path.as_str(),
        "type": status_warning_type_wire(warning.warning_type),
        "detected_at": time.realtime_timestamp(warning.detected_at_ns),
    })
}

fn status_warning_type_wire(warning_type: ServiceStatusWarningType) -> &'static str {
    match warning_type {
        ServiceStatusWarningType::ServiceTree => "service_tree",
        ServiceStatusWarningType::Health => "health",
        ServiceStatusWarningType::Hooks => "hooks",
        ServiceStatusWarningType::Helper => "helper",
    }
}

pub fn control_list_response_line(
    services: &[ServiceListItem],
) -> Result<Vec<u8>, serde_json::Error> {
    let services = services
        .iter()
        .map(|item| {
            json!({
                "service": item.service.as_str(),
                "display_name": item.display_name.as_deref(),
                "description": item.description.as_deref(),
                "state": service_state_wire(item.state),
                "cause": item.cause.map(transition_cause_wire),
                "health": item.health.map(service_health_wire),
            })
        })
        .collect::<Vec<_>>();
    let response = json!({
        "status": ControlResponseStatus::Ok.as_str(),
        "services": services,
    });
    response_line(&response)
}

pub fn control_operation_status_response_line(
    view: &OperationStatusView,
    time: ControlResponseTimeProjection,
) -> Result<Vec<u8>, serde_json::Error> {
    let response = json!({
        "status": ControlResponseStatus::Ok.as_str(),
        "operation": {
            "id": view.id.to_canonical_string(),
            "type": operation_type_wire(view.operation_type),
            "service": view.service.as_str(),
            "source": operation_source_wire(view.source),
            "state": operation_state_wire(view.state),
            "result": view.result.as_deref(),
            "merged_into": view.merged_into.map(OperationId::to_canonical_string),
            "error": view.error.as_deref(),
            "requested_at": time.realtime_timestamp(view.created_at_ns),
            "started_at": view.started_at_ns.map(|started_at_ns| time.realtime_timestamp(started_at_ns)),
            "completed_at": view.completed_at_ns.map(|completed_at_ns| time.realtime_timestamp(completed_at_ns)),
        }
    });
    response_line(&response)
}

pub fn control_lifecycle_ack_response_line(
    operation_id: Option<OperationId>,
    service: &str,
    state: ServiceState,
    cause: Option<TransitionCause>,
    warnings: &[String],
) -> Result<Vec<u8>, serde_json::Error> {
    control_lifecycle_ack_response_line_with_mode(
        operation_id,
        service,
        state,
        cause,
        warnings,
        None,
    )
}

pub fn control_lifecycle_ack_response_line_with_mode(
    operation_id: Option<OperationId>,
    service: &str,
    state: ServiceState,
    cause: Option<TransitionCause>,
    warnings: &[String],
    mode: Option<&str>,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut response = json!({
        "status": ControlResponseStatus::Ok.as_str(),
        "operation_id": operation_id.map(OperationId::to_canonical_string),
        "service": service,
        "state": service_state_wire(state),
        "cause": cause.map(transition_cause_wire),
        "warnings": warnings,
    });
    if let Some(mode) = mode {
        response["mode"] = json!(mode);
    }
    response_line(&response)
}

pub fn control_reload_config_response_line(
    outcome: &ReloadConfigOutcome,
) -> Result<Vec<u8>, serde_json::Error> {
    let response = json!({
        "status": ControlResponseStatus::Ok.as_str(),
        "summary": {
            "added": &outcome.summary.added,
            "updated": &outcome.summary.updated,
            "restored": &outcome.summary.restored,
            "marked_removed": &outcome.summary.marked_removed,
            "discarded": &outcome.summary.discarded,
            "undecodable": &outcome.summary.undecodable,
        },
        // Which key, which field, and why: the detail the operator needs to
        // repair it, which "INTERNAL_ERROR: control request failed" withheld
        // (PEI-621).
        "undecodable": outcome.undecodable.iter().map(|service| {
            json!({
                "service": service.name.as_str(),
                "field": service.field.as_deref(),
                "message": service.message.as_str(),
            })
        }).collect::<Vec<_>>(),
        "warnings": outcome.warning_messages(),
    });
    response_line(&response)
}

pub fn control_error_response_line(
    code: ControlErrorCode,
    message: &str,
) -> Result<Vec<u8>, serde_json::Error> {
    let response = json!({
        "status": ControlResponseStatus::Error.as_str(),
        "code": code.as_str(),
        "message": message,
    });
    response_line(&response)
}

/// One frame from an arbitrary object, for response shapes built elsewhere.
pub fn response_line_from_value(response: serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
    response_line(&response)
}

fn response_line(response: &serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
    let mut line = serde_json::to_vec(response)?;
    line.push(b'\n');
    Ok(line)
}
