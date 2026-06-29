use crate::supervisor::SupervisorControlCommandBodyResponse;

use super::super::super::{LIFECYCLE_COMMAND_NS, ScriptedClock, TestProcessController};
use super::super::support::{
    TestAccessChecker, body_context, booted_supervisor, control_peer, inactive_alive_service,
    response_json,
};

#[test]
fn reset_abandoned_populated_cgroup_returns_lifecycle_warning() {
    let mut supervisor = abandoned_supervisor("app");
    let mut access = TestAccessChecker::allow_all();
    let mut controller = TestProcessController::default();
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/app/main", true);
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    let peer = control_peer();

    let response = supervisor
        .run_checked_control_body_with_response(
            br#"{"command":"reset","service":"app","wait":false}"#,
            body_context(&peer, &mut access, &mut controller, &mut clock),
        )
        .expect("reset response");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: Some(response_line),
        wait: None,
        ..
    } = response
    else {
        panic!("expected reset ack");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "app");
    assert_eq!(json["state"], "inactive");
    assert_eq!(
        json["warnings"],
        serde_json::json!([
            "abandoned main cgroup for service app is still populated after reset -- cgroup remains leaked; underlying D-state process requires investigation"
        ])
    );
    assert_eq!(
        controller.cgroup_populated_checks,
        vec!["/sys/fs/cgroup/peinit/app/main"]
    );
    assert!(controller.cgroup_removes.is_empty());
}

fn abandoned_supervisor(service: &str) -> crate::supervisor::Supervisor {
    let mut supervisor = booted_supervisor(vec![inactive_alive_service(service)]);
    supervisor
        .services
        .transition_service(
            service,
            crate::service::runtime::ServiceTransition {
                to: crate::service::runtime::ServiceState::Starting,
                cause: crate::service::runtime::TransitionCause::ExplicitStart,
            },
        )
        .expect("starting service");
    supervisor
        .services
        .transition_service(
            service,
            crate::service::runtime::ServiceTransition {
                to: crate::service::runtime::ServiceState::Stopping,
                cause: crate::service::runtime::TransitionCause::ExplicitStop,
            },
        )
        .expect("stopping service");
    supervisor
        .services
        .transition_service(
            service,
            crate::service::runtime::ServiceTransition {
                to: crate::service::runtime::ServiceState::Abandoned,
                cause: crate::service::runtime::TransitionCause::ProcessUnkillable,
            },
        )
        .expect("abandoned service");
    supervisor
}
