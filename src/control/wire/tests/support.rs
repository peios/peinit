pub(super) use crate::control::query::{
    CurrentJobView, CurrentOperationView, OperationStatusView, ServiceListItem, ServiceStatusView,
    ServiceStatusWarning, ServiceStatusWarningType,
};
pub(super) use crate::control::reload_config::ReloadConfigOutcome;
pub(super) use crate::control::socket::ControlSocketLimits;
pub(super) use crate::control::system::ControlSecurityDescriptor;
pub(super) use crate::ids::{JobId, JobIdAllocator, OperationId, OperationIdAllocator};
pub(super) use crate::job::JobType;
pub(super) use crate::logging::RuntimeLogConfig;
pub(super) use crate::operation::{OperationSource, OperationState, OperationType};
pub(super) use crate::registry::RegistryConfigWarning;
pub(super) use crate::service::runtime::{ServiceHealthStatus, ServiceState, TransitionCause};
pub(super) use crate::service::{ServiceGraphWarning, ServiceReloadSummary};
pub(super) use crate::shutdown::ShutdownKind;
pub(super) use crate::shutdown::ShutdownSettings;

pub(super) use super::super::{
    ControlCommand, ControlErrorCode, ControlFrameDecision, ControlFrameRejectReason,
    ControlRequestParseError, ControlResponseStatus, ControlResponseTimeProjection,
    ParsedControlRequest, control_error_response_line, control_frame_decision,
    control_lifecycle_ack_response_line_with_mode, control_list_response_line,
    control_operation_status_response_line, control_reload_config_response_line,
    control_status_response_line, control_system_ok_response_line, parse_control_request,
};

pub(super) fn shutdown_kind(body: &[u8]) -> ShutdownKind {
    parse_control_request(body)
        .expect("shutdown request")
        .shutdown_kind
        .expect("shutdown kind")
}

pub(super) fn response_time() -> ControlResponseTimeProjection {
    ControlResponseTimeProjection::new(10_000_000_000, 1_717_171_717_123_456_789)
}

pub(super) fn response_json(line: &[u8]) -> serde_json::Value {
    assert_eq!(line.last(), Some(&b'\n'));
    serde_json::from_slice(&line[..line.len() - 1]).expect("response json")
}

pub(super) fn sorted_keys(value: &serde_json::Value) -> Vec<&str> {
    let mut keys = value
        .as_object()
        .expect("json object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys
}

pub(super) fn operation_id(sequence: u64) -> OperationId {
    OperationIdAllocator::with_next_sequence(sequence)
        .allocate_batch(1, 1_717_171_717_123_456_789)
        .expect("operation id")[0]
}

pub(super) fn job_id(_sequence: u64) -> JobId {
    JobIdAllocator::new()
        .allocate_batch(1, 1_717_171_717_123_456_789)
        .expect("job id")[0]
}

pub(super) fn reload_config_outcome() -> ReloadConfigOutcome {
    ReloadConfigOutcome {
        summary: ServiceReloadSummary {
            added: vec!["new".to_string()],
            updated: Vec::new(),
            restored: Vec::new(),
            marked_removed: Vec::new(),
            discarded: Vec::new(),
        },
        services_schema_version: 2,
        config_warnings: vec![RegistryConfigWarning::NewerServicesSchemaVersion {
            observed: 2,
            supported: 1,
        }],
        control_security: ControlSecurityDescriptor::Default,
        control_limits: ControlSocketLimits::default(),
        log_config: RuntimeLogConfig::default(),
        shutdown_settings: ShutdownSettings::default(),
        global_environment: Vec::new(),
        eventd_log_socket_path: None,
        warnings: vec![ServiceGraphWarning::AliveReadinessWithHardDependents {
            service: "app".to_string(),
            dependents: vec!["api".to_string()],
        }],
    }
}
