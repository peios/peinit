use crate::service::runtime::{ServiceHealthStatus, ServiceState, TransitionCause};
use crate::supervisor::SupervisorHealthCheckOutcome;
use crate::supervisor::tests::TestProcessController;

use super::support::{NoopFinalizer, active_health_supervisor, launch_due_health_check};

#[test]
fn successful_health_check_marks_service_healthy_and_kills_probe_cgroup() {
    let (mut supervisor, first_due) = active_health_supervisor(3);
    let launch = launch_due_health_check(&mut supervisor, first_due, 9000, 70);
    let mut controller = TestProcessController::default();
    let mut finalizer = NoopFinalizer;

    let terminal = supervisor
        .complete_health_check_job_with_shutdown_finalizer(
            launch.launch.job_id,
            first_due + 100_000,
            0,
            &mut controller,
            &mut finalizer,
        )
        .expect("health terminal");

    assert_eq!(terminal.outcome, SupervisorHealthCheckOutcome::Healthy);
    assert!(terminal.service_job_event.is_none());
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/health".to_string()],
    );
    assert_eq!(
        supervisor.service_status("app").expect("status").health,
        Some(ServiceHealthStatus::Healthy),
    );
    assert!(supervisor.next_health_check_timeout().is_none());
}

#[test]
fn failed_health_check_below_retry_limit_marks_unhealthy_without_restart() {
    let (mut supervisor, first_due) = active_health_supervisor(3);
    let launch = launch_due_health_check(&mut supervisor, first_due, 9000, 70);
    let mut controller = TestProcessController::default();
    let mut finalizer = NoopFinalizer;

    let terminal = supervisor
        .complete_health_check_job_with_shutdown_finalizer(
            launch.launch.job_id,
            first_due + 100_000,
            1,
            &mut controller,
            &mut finalizer,
        )
        .expect("health terminal");

    assert_eq!(
        terminal.outcome,
        SupervisorHealthCheckOutcome::Unhealthy {
            consecutive_failures: 1,
            retries: 3,
        },
    );
    assert!(terminal.service_job_event.is_none());
    let status = supervisor.service_status("app").expect("status");
    assert_eq!(status.state, ServiceState::Active);
    assert_eq!(status.health, Some(ServiceHealthStatus::Unhealthy));
    assert!(supervisor.next_restart_backoff_deadline().is_none());
}

#[test]
fn failed_health_check_at_retry_limit_enters_restart_backoff() {
    let (mut supervisor, first_due) = active_health_supervisor(1);
    let launch = launch_due_health_check(&mut supervisor, first_due, 9000, 70);
    let mut controller = TestProcessController::default();
    let mut finalizer = NoopFinalizer;

    let terminal = supervisor
        .complete_health_check_job_with_shutdown_finalizer(
            launch.launch.job_id,
            first_due + 100_000,
            1,
            &mut controller,
            &mut finalizer,
        )
        .expect("health terminal");

    assert_eq!(
        terminal.outcome,
        SupervisorHealthCheckOutcome::RestartScheduled {
            consecutive_failures: 1,
            retries: 1,
        },
    );
    assert!(terminal.service_job_event.is_some());
    assert_eq!(
        terminal.service_transitions[0].event.cause,
        TransitionCause::HealthCheckFailure,
    );
    assert_eq!(
        controller.cgroup_kills,
        vec![
            "/sys/fs/cgroup/peinit/app/health".to_string(),
            "/sys/fs/cgroup/peinit/app".to_string(),
        ],
    );
    let status = supervisor.service_status("app").expect("status");
    assert_eq!(status.state, ServiceState::Backoff);
    assert_eq!(status.health, Some(ServiceHealthStatus::Unhealthy));
    assert_eq!(
        supervisor
            .next_restart_backoff_deadline()
            .expect("restart backoff")
            .service,
        "app",
    );
    assert!(supervisor.jobs().current_service_main_job("app").is_none());
    assert!(supervisor.next_health_check_interval().is_none());
}
