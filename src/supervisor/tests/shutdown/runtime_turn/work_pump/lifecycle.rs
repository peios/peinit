use crate::runtime::RuntimeEventSource;
use crate::service::runtime::ServiceState;
use crate::supervisor::tests::{APP_CRASH_NS, RESTART_LAUNCH_NS, alive_service, process};

use super::support::{FakeDeadlineTimer, LoopScript, active_app_supervisor, run_loop};

#[test]
fn runtime_pump_launches_restart_after_lifecycle_deadline_event() {
    let mut supervisor = active_app_supervisor(alive_service("app"), process(5000, 20));
    let job_id = supervisor
        .service_status("app")
        .expect("app")
        .current_job
        .expect("app job")
        .id;
    supervisor
        .complete_job(job_id, APP_CRASH_NS, 1)
        .expect("crash app");
    let deadline = supervisor
        .next_restart_backoff_deadline()
        .expect("restart backoff");

    let result = run_loop(
        &mut supervisor,
        LoopScript::new(
            [deadline.due_at_ns, RESTART_LAUNCH_NS],
            vec![process(5001, 21)],
        )
        .events([RuntimeEventSource::LifecycleDeadlineTimer])
        .lifecycle_timer(FakeDeadlineTimer::expired_once()),
    );

    assert!(result.turn.pre_work.is_empty());
    assert_eq!(result.turn.post_work.service_launches.len(), 1);
    assert_eq!(
        result.turn.post_work.service_launches[0].launch.process.pid,
        5001,
    );
    let app = supervisor.service_status("app").expect("app");
    assert_eq!(app.state, ServiceState::Active);
    assert_eq!(app.generation, 2);
    assert!(supervisor.pending_launch_jobs().is_empty());
}
