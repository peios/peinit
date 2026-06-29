use crate::runtime::{RuntimeWorkPumpConfig, RuntimeWorkPumpContext, drain_runtime_work_queues};
use crate::supervisor::SupervisorControlCommandBodyResponse;

use super::super::{
    LIFECYCLE_COMMAND_NS, ScriptedClock, TestProcessController, TestProcessLauncher,
    TestTokenProvider, process,
};
use super::support::{
    TestAccessChecker, body_context, booted_supervisor, control_peer, inactive_alive_service,
    response_json,
};

#[test]
fn status_projects_current_job_time_and_uptime_from_realtime_clock() {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service("app")]);
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS + 5_000_000_000,
    ]);
    let peer = control_peer();

    let start_response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"start","service":"app","wait":false}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("start response");
    assert!(matches!(
        start_response,
        SupervisorControlCommandBodyResponse::Accepted { .. }
    ));

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9000, 90)]);
    let mut filesystem_check_launcher =
        crate::supervisor::tests::TestFilesystemCheckLauncher::default();
    drain_runtime_work_queues(
        &mut supervisor,
        &mut RuntimeWorkPumpContext {
            clock: &mut clock,
            controller: &mut controller,
            token_provider: &mut tokens,
            process_launcher: &mut launcher,
            filesystem_check_launcher: &mut filesystem_check_launcher,
            config: RuntimeWorkPumpConfig::default(),
        },
    )
    .expect("drain work");

    let status_response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"status","service":"app"}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("status response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        ..
    } = status_response
    else {
        panic!("expected status response");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "app");
    assert_eq!(json["state"], "active");
    assert_eq!(json["uptime_seconds"], 5);
    assert!(json["warnings"].as_array().expect("warnings").is_empty());
    assert_eq!(
        json["current_job"]["started_at"],
        "2024-05-31T16:08:32.123456789Z"
    );
}
