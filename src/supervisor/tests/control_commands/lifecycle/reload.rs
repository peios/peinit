use crate::control::connection::{ControlConnectionRecord, ControlConnectionTable};
use crate::control::socket::ControlSocketRead;
use crate::execution::control::ControlExecutionDetail;
use crate::service::{Readiness, ServiceDefinition};
use crate::supervisor::{SupervisorControlConnectionTurnContext, SupervisorControlFrameTurn};

use super::super::super::{
    LIFECYCLE_COMMAND_NS, ScriptedClock, TestProcessController, TestProcessLauncher,
    TestTokenProvider, process,
};
use super::super::support::{
    DEFAULT_CONTROL_SECURITY, FakeConnectionIo, TestAccessChecker, booted_supervisor, control_peer,
    response_json,
};

#[test]
fn failed_reload_wait_response_includes_failed_mode() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.readiness = Readiness::Alive;
    app.exec_reload = Some("/usr/bin/reload".to_string());
    let mut supervisor = booted_supervisor(vec![app]);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9000, 90), process(9001, 91)]);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([
        LIFECYCLE_COMMAND_NS,
        LIFECYCLE_COMMAND_NS + 1,
        LIFECYCLE_COMMAND_NS + 2,
        LIFECYCLE_COMMAND_NS + 3,
        LIFECYCLE_COMMAND_NS + 4,
        LIFECYCLE_COMMAND_NS + 5,
    ]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch");

    let mut access = TestAccessChecker::allow_all();
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            44,
            ControlConnectionRecord::new(
                FakeConnectionIo::scripted_reads([ControlSocketRead::Bytes(
                    b"{\"command\":\"reload\",\"service\":\"app\",\"wait\":true}\n".to_vec(),
                )]),
                control_peer(),
            ),
        )
        .expect("admit connection");

    let turn = supervisor
        .process_control_connection_table_turn(
            &mut connections,
            44,
            SupervisorControlConnectionTurnContext {
                control_security: &DEFAULT_CONTROL_SECURITY,
                access_checker: &mut access,
                controller: &mut controller,
                clock: &mut clock,
                registry: None,
                max_read_bytes: 1024,
                max_request_bytes: crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES,
                observed_at_ns: LIFECYCLE_COMMAND_NS,
            },
        )
        .expect("connection turn");
    let SupervisorControlFrameTurn::CommandAccepted {
        wait: Some(wait), ..
    } = turn.turn.frame.expect("frame").frame
    else {
        panic!("expected reload wait");
    };

    let execution = supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload execution");
    let ControlExecutionDetail::ReloadCommand { job_id, .. } = execution.execution.detail else {
        panic!("expected reload hook");
    };
    supervisor
        .launch_next_pending_control_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch reload hook")
        .expect("reload hook launch");
    supervisor
        .complete_reload_command_job(job_id, LIFECYCLE_COMMAND_NS + 3, 2, &mut controller)
        .expect("fail reload hook");

    let flush = supervisor
        .flush_terminal_control_waits(&mut connections, LIFECYCLE_COMMAND_NS + 3)
        .expect("flush waits");

    assert_eq!(flush.completed.len(), 1);
    let record = connections.get(44).expect("connection");
    assert!(record.state().pending_wait().is_none());
    let writes = record.io().writes.borrow();
    assert_eq!(writes.len(), 1);
    let json = response_json(&writes[0]);
    assert_eq!(json["status"], "ok");
    assert_eq!(
        json["operation_id"],
        wait.operation_id.to_canonical_string()
    );
    assert_eq!(json["service"], "app");
    assert_eq!(json["state"], "active");
    assert_eq!(json["mode"], "failed");
}
