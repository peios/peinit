use super::support::*;

#[test]
fn serializes_ok_response_as_newline_delimited_json() {
    let line = control_system_ok_response_line().expect("response");
    assert_eq!(line.last(), Some(&b'\n'));

    let response: serde_json::Value =
        serde_json::from_slice(&line[..line.len() - 1]).expect("response json");
    assert_eq!(response["status"], "ok");
}

#[test]
fn serializes_error_response_with_canonical_code_and_message() {
    let line = control_error_response_line(ControlErrorCode::AccessDenied, "caller lacks shutdown")
        .expect("response");
    assert_eq!(line.last(), Some(&b'\n'));

    let response: serde_json::Value =
        serde_json::from_slice(&line[..line.len() - 1]).expect("response json");
    assert_eq!(response["status"], "error");
    assert_eq!(response["code"], "ACCESS_DENIED");
    assert_eq!(response["message"], "caller lacks shutdown");
}

#[test]
fn serializes_status_response_shape_with_structured_warnings_and_timestamps() {
    let operation_id = operation_id(2);
    let view = ServiceStatusView {
        service: "app".to_string(),
        state: ServiceState::Active,
        cause: Some(TransitionCause::ExplicitStart),
        generation: 99,
        status_text: Some("ready".to_string()),
        health: Some(ServiceHealthStatus::Healthy),
        definition_removed: true,
        current_job: Some(CurrentJobView {
            id: job_id(1),
            job_type: JobType::ServiceMain,
            pid: Some(1234),
            started_at_ns: Some(9_000_000_000),
            identity: "LocalService".to_string(),
        }),
        current_operation: Some(CurrentOperationView {
            id: operation_id,
            operation_type: OperationType::Start,
            source: OperationSource::Admin,
            state: OperationState::Running,
        }),
        warnings: vec![ServiceStatusWarning {
            path: "/sys/fs/cgroup/peinit/app.gen1/health".to_string(),
            warning_type: ServiceStatusWarningType::Health,
            detected_at_ns: 10_000_000_000,
        }],
        lifecycle_warnings: Vec::new(),
    };

    let line = control_status_response_line(&view, response_time()).expect("status response");
    let response = response_json(&line);

    assert_eq!(
        sorted_keys(&response),
        [
            "cause",
            "current_job",
            "current_operation",
            "definition_removed",
            "health",
            "service",
            "state",
            "status",
            "status_text",
            "uptime_seconds",
            "warnings",
        ],
    );
    assert_eq!(response["status"], "ok");
    assert_eq!(response["state"], "active");
    assert_eq!(response["cause"], "explicit_start");
    assert_eq!(response["definition_removed"], true);
    assert_eq!(response["current_job"]["type"], "service_main");
    assert_eq!(
        response["current_job"]["started_at"],
        "2024-05-31T16:08:36.123456789Z",
    );
    assert_eq!(response["uptime_seconds"], 1);
    assert_eq!(
        sorted_keys(&response["current_operation"]),
        ["id", "source", "type"],
    );
    assert_eq!(
        response["current_operation"]["id"],
        operation_id.to_canonical_string()
    );
    assert_eq!(
        response["warnings"][0]["path"],
        "/sys/fs/cgroup/peinit/app.gen1/health"
    );
    assert_eq!(response["warnings"][0]["type"], "health");
    assert_eq!(
        response["warnings"][0]["detected_at"],
        "2024-05-31T16:08:37.123456789Z",
    );
}

#[test]
fn serializes_list_response_as_compact_query_authorized_summaries() {
    let services = vec![ServiceListItem {
        service: "app".to_string(),
        state: ServiceState::Active,
        cause: Some(TransitionCause::ExplicitStart),
        health: None,
        definition_removed: true,
    }];

    let line = control_list_response_line(&services).expect("list response");
    let response = response_json(&line);

    assert_eq!(sorted_keys(&response), ["services", "status"]);
    assert_eq!(
        sorted_keys(&response["services"][0]),
        ["cause", "health", "service", "state"],
    );
    assert_eq!(response["services"][0]["service"], "app");
    assert_eq!(response["services"][0]["state"], "active");
    assert_eq!(response["services"][0]["cause"], "explicit_start");
    assert!(response["services"][0]["health"].is_null());
}

#[test]
fn serializes_operation_status_response_with_nulls_and_rfc3339_timestamps() {
    let id = operation_id(3);
    let view = OperationStatusView {
        id,
        operation_type: OperationType::Start,
        service: "app".to_string(),
        source: OperationSource::Admin,
        state: OperationState::Completed,
        created_at_ns: 8_000_000_000,
        started_at_ns: None,
        completed_at_ns: Some(10_000_000_000),
        result: Some("active".to_string()),
        error: None,
        merged_into: None,
    };

    let line =
        control_operation_status_response_line(&view, response_time()).expect("operation response");
    let response = response_json(&line);

    assert_eq!(response["status"], "ok");
    assert_eq!(
        sorted_keys(&response["operation"]),
        [
            "completed_at",
            "error",
            "id",
            "merged_into",
            "requested_at",
            "result",
            "service",
            "source",
            "started_at",
            "state",
            "type",
        ],
    );
    assert_eq!(response["operation"]["id"], id.to_canonical_string());
    assert_eq!(response["operation"]["state"], "completed");
    assert_eq!(response["operation"]["result"], "active");
    assert!(response["operation"]["error"].is_null());
    assert!(response["operation"]["merged_into"].is_null());
    assert!(response["operation"]["started_at"].is_null());
    assert_eq!(
        response["operation"]["requested_at"],
        "2024-05-31T16:08:35.123456789Z",
    );
    assert_eq!(
        response["operation"]["completed_at"],
        "2024-05-31T16:08:37.123456789Z",
    );
}

#[test]
fn serializes_lifecycle_and_reload_config_warning_arrays() {
    let operation_id = operation_id(4);
    let lifecycle = response_json(
        &control_lifecycle_ack_response_line_with_mode(
            Some(operation_id),
            "app",
            ServiceState::Active,
            Some(TransitionCause::ExplicitStart),
            &["service has leaked sub-cgroups from a previous generation -- indicates underlying I/O problem requiring investigation".to_string()],
            Some("confirmed"),
        )
        .expect("lifecycle response"),
    );
    assert_eq!(lifecycle["status"], "ok");
    assert_eq!(
        lifecycle["operation_id"],
        operation_id.to_canonical_string()
    );
    assert_eq!(lifecycle["warnings"].as_array().expect("warnings").len(), 1);
    assert_eq!(lifecycle["mode"], "confirmed");

    let reload = response_json(
        &control_reload_config_response_line(&reload_config_outcome()).expect("reload response"),
    );
    assert_eq!(reload["status"], "ok");
    assert_eq!(reload["summary"]["added"], serde_json::json!(["new"]));
    assert_eq!(reload["warnings"].as_array().expect("warnings").len(), 2);
}
