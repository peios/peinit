use crate::execution::control::ControlExecutionDetail;
use crate::job::JobType;
use crate::operation::OperationState;
use crate::service::runtime::ServiceState;
use crate::supervisor::tests::{alive_service, process};

use super::support::{
    CONTROL_NS, LoopScript, RELOAD_COMMAND_LAUNCH_NS, active_app_supervisor, queue_reload, run_loop,
};

#[test]
fn runtime_pump_executes_reload_operation_and_launches_reload_job() {
    let mut app = alive_service("app");
    app.exec_reload = Some(r#"/bin/reload --name="hello world" """#.to_string());
    app.start_timeout_secs = 45;
    let mut supervisor = active_app_supervisor(app, process(8000, 50));
    let operation_id = queue_reload(&mut supervisor);

    let result = run_loop(
        &mut supervisor,
        LoopScript::new(
            [CONTROL_NS, RELOAD_COMMAND_LAUNCH_NS],
            vec![process(9100, 61)],
        ),
    );

    assert_eq!(result.turn.pre_work.control_operations.len(), 1);
    assert_eq!(result.turn.pre_work.control_launches.len(), 1);
    assert!(result.turn.post_work.is_empty());
    let control = &result.turn.pre_work.control_operations[0].execution;
    let ControlExecutionDetail::ReloadCommand { job_id, job_event } = &control.detail else {
        panic!("expected reload command");
    };
    assert_eq!(job_event.job_type, JobType::ReloadHook);
    assert_eq!(
        result.turn.pre_work.control_launches[0].launch.job_id,
        *job_id,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Reloading,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("reload operation")
            .state,
        OperationState::Running,
    );
    assert!(supervisor.pending_control_operations().is_empty());
    assert!(supervisor.pending_control_launch_jobs().is_empty());
}
