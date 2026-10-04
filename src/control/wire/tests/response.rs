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
        display_name: Some("Application".to_string()),
        description: Some("An example application".to_string()),
        state: ServiceState::Active,
        cause: Some(TransitionCause::ExplicitStart),
        generation: 99,
        status_text: Some("ready".to_string()),
        progress: None,
        progress_unit: None,
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
        timers: vec![
            ServiceTimerView {
                schedule: "*-*-* 00:00:00".to_string(),
                arming: ServiceTimerArming::Armed {
                    scheduled_ns: 1_717_200_000_000_000_000,
                    fires_ns: 1_717_200_090_000_000_000,
                    last_fired_ns: None,
                },
            },
            ServiceTimerView {
                schedule: "*-02-30".to_string(),
                arming: ServiceTimerArming::NotArmed {
                    reason: "calendar expression has no future occurrence".to_string(),
                },
            },
        ],
    };

    // Given out of order and with a bit that is no service right, as an
    // AccessCheck's granted mask may be: `granted` names only the service
    // rights, in §4.7's order.
    let granted = ServiceAccess::from_granted_bits(
        ServiceAccess::INTERROGATE.bits() | ServiceAccess::QUERY_STATUS.bits() | 0x0002_0000,
    );
    let line =
        control_status_response_line(&view, granted, response_time()).expect("status response");
    let response = response_json(&line);

    assert_eq!(
        sorted_keys(&response),
        [
            "cause",
            "current_job",
            "current_operation",
            "definition_removed",
            "description",
            "display_name",
            "granted",
            "health",
            "progress",
            "service",
            "state",
            "status",
            "status_text",
            "timers",
            "uptime_seconds",
            "warnings",
        ],
    );
    // Not applicable is present as null, not omitted (§4.9).
    assert!(response["progress"].is_null());
    // A timer's times are the wall clock's already, not projected.
    assert_eq!(
        sorted_keys(&response["timers"][0]),
        ["fires_at", "last_fired_at", "not_armed", "schedule", "scheduled_at"],
    );
    assert_eq!(response["timers"][0]["schedule"], "*-*-* 00:00:00");
    assert_eq!(response["timers"][0]["scheduled_at"], "2024-06-01T00:00:00.000000000Z");
    assert_eq!(response["timers"][0]["fires_at"], "2024-06-01T00:01:30.000000000Z");
    assert!(response["timers"][0]["last_fired_at"].is_null());
    assert!(response["timers"][0]["not_armed"].is_null());
    assert!(response["timers"][1]["fires_at"].is_null());
    assert_eq!(
        response["timers"][1]["not_armed"],
        "calendar expression has no future occurrence"
    );
    assert_eq!(response["status"], "ok");
    assert_eq!(response["display_name"], "Application");
    assert_eq!(response["description"], "An example application");
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
    assert_eq!(
        response["granted"],
        serde_json::json!(["query_status", "interrogate"])
    );
}

/// A service's retained progress is reported in the job view's form
/// (§4.14, §7.7): `total` null for `N` and `N/`, `bounded` telling them
/// apart, and `unit` null until a `PROGRESS_UNIT` arrives.
#[test]
fn status_reports_progress_in_the_job_view_form() {
    let view = |progress, progress_unit| ServiceStatusView {
        service: "indexer".to_string(),
        display_name: None,
        description: None,
        state: ServiceState::Starting,
        cause: Some(TransitionCause::ExplicitStart),
        generation: 1,
        status_text: Some("Scanning".to_string()),
        progress,
        progress_unit,
        health: None,
        definition_removed: false,
        current_job: None,
        current_operation: None,
        warnings: Vec::new(),
        lifecycle_warnings: Vec::new(),
        timers: Vec::new(),
    };
    let progress_of = |view: &ServiceStatusView| {
        let line = control_status_response_line(view, ServiceAccess::ALL, response_time())
            .expect("status response");
        response_json(&line)["progress"].clone()
    };

    assert_eq!(
        progress_of(&view(
            Some(JobProgress {
                current: 3,
                total: Some(10),
                bounded: true,
            }),
            Some(JobProgressUnit::Items),
        )),
        serde_json::json!({"current": 3, "total": 10, "bounded": true, "unit": "items"}),
    );
    assert_eq!(
        progress_of(&view(
            Some(JobProgress {
                current: 7,
                total: None,
                bounded: true,
            }),
            None,
        )),
        serde_json::json!({"current": 7, "total": null, "bounded": true, "unit": null}),
    );
    assert_eq!(
        progress_of(&view(
            Some(JobProgress {
                current: 42,
                total: None,
                bounded: false,
            }),
            Some(JobProgressUnit::Bytes),
        )),
        serde_json::json!({"current": 42, "total": null, "bounded": false, "unit": "bytes"}),
    );
    // A unit with no figure is no progress.
    assert!(progress_of(&view(None, Some(JobProgressUnit::Percent))).is_null());
}

/// A caller holding every service right is told all four, in §4.7's
/// order, and one holding none gets an empty array rather than null.
#[test]
fn status_granted_lists_every_right_held_in_order_and_is_never_null() {
    assert_eq!(
        ServiceAccess::ALL.wire_names(),
        ["query_status", "start", "stop", "interrogate"]
    );
    assert!(
        ServiceAccess::from_granted_bits(0x0002_0000)
            .wire_names()
            .is_empty()
    );
}

#[test]
fn serializes_list_response_as_compact_query_authorized_summaries() {
    let services = vec![ServiceListItem {
        service: "app".to_string(),
        display_name: Some("Application".to_string()),
        description: None,
        state: ServiceState::Active,
        cause: Some(TransitionCause::ExplicitStart),
        health: None,
        definition_removed: true,
        next_timer_ns: Some(1_717_200_090_000_000_000),
    }];

    let line = control_list_response_line(&services).expect("list response");
    let response = response_json(&line);

    assert_eq!(sorted_keys(&response), ["services", "status"]);
    assert_eq!(
        sorted_keys(&response["services"][0]),
        [
            "cause",
            "description",
            "display_name",
            "health",
            "next_timer_at",
            "service",
            "state"
        ],
    );
    assert_eq!(
        response["services"][0]["next_timer_at"],
        "2024-06-01T00:01:30.000000000Z"
    );
    assert_eq!(response["services"][0]["service"], "app");
    assert_eq!(response["services"][0]["display_name"], "Application");
    assert!(response["services"][0]["description"].is_null());
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
fn serializes_boot_response_with_every_field_and_a_projected_time() {
    let view = BootStatusView {
        mode: BootMode::Safe,
        reason: BootModeReason::SafeModeDowngrade,
        downgrade: vec!["critical service in dependency cycle a -> b".to_string()],
        attempts: 1,
        max_attempts: 3,
        confirmed: false,
        grace_seconds: 30,
        waiting_on: Vec::new(),
        confirms_at_ns: Some(12_000_000_000),
        confirm_error: None,
    };

    let response =
        response_json(&control_boot_response_line(&view, response_time()).expect("boot response"));

    assert_eq!(response["status"], "ok");
    assert_eq!(
        sorted_keys(&response["boot"]),
        [
            "attempts",
            "confirm_error",
            "confirmed",
            "confirms_at",
            "downgrade",
            "grace_seconds",
            "max_attempts",
            "mode",
            "reason",
            "waiting_on",
        ],
    );
    assert_eq!(response["boot"]["mode"], "safe");
    assert_eq!(response["boot"]["reason"], "safe_mode_downgrade");
    assert_eq!(
        response["boot"]["downgrade"],
        serde_json::json!(["critical service in dependency cycle a -> b"]),
    );
    assert_eq!(response["boot"]["waiting_on"], serde_json::json!([]));
    assert_eq!(
        response["boot"]["confirms_at"],
        "2024-05-31T16:08:39.123456789Z"
    );
    assert!(response["boot"]["confirm_error"].is_null());
}

#[test]
fn boot_mode_and_reason_labels_are_lower_snake_case() {
    assert_eq!(BootMode::Full.wire(), "full");
    assert_eq!(BootMode::Safe.wire(), "safe");
    assert_eq!(BootMode::Recovery.wire(), "recovery");
    assert_eq!(BootModeReason::Normal.wire(), "normal");
    assert_eq!(BootModeReason::Requested.wire(), "requested");
    assert_eq!(
        BootModeReason::SafeModeDowngrade.wire(),
        "safe_mode_downgrade"
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
