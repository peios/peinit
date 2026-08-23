use crate::control::query::ServiceStatusWarningType;
use crate::service::runtime::ServiceState;
use crate::supervisor::SupervisorHealthCheckOutcome;
use crate::supervisor::tests::TestProcessController;

use super::support::{active_health_supervisor, launch_due_health_check};

#[test]
fn health_check_timeout_kills_cgroup_and_counts_as_failure() {
    let (mut supervisor, first_due) = active_health_supervisor(1);
    let launch = launch_due_health_check(&mut supervisor, first_due, 9000, 70);
    let timeout = supervisor
        .next_health_check_timeout()
        .expect("health timeout")
        .due_at_ns;
    let mut controller = TestProcessController::default();

    let timeouts = supervisor
        .process_due_health_check_timeouts(&mut controller, timeout)
        .expect("health timeout");

    assert_eq!(timeouts.len(), 1);
    assert_eq!(timeouts[0].terminal.job_event.job_id, launch.launch.job_id);
    assert_eq!(
        timeouts[0].terminal.outcome,
        SupervisorHealthCheckOutcome::RestartScheduled {
            consecutive_failures: 1,
            retries: 1,
        },
    );
    assert!(timeouts[0].terminal.service_job_event.is_some());
    assert_eq!(
        controller.cgroup_kills,
        vec![
            "/sys/fs/cgroup/peinit/app/health".to_string(),
            "/sys/fs/cgroup/peinit/app".to_string(),
        ],
    );
    assert_eq!(
        supervisor.service_status("app").expect("status").state,
        ServiceState::Backoff,
    );
    assert!(supervisor.jobs().current_service_main_job("app").is_none());
}

#[test]
fn health_check_timeout_records_leaked_sub_cgroup_warning_after_post_kill_timeout() {
    let (mut supervisor, first_due) = active_health_supervisor(1);
    launch_due_health_check(&mut supervisor, first_due, 9000, 70);
    let timeout = supervisor
        .next_health_check_timeout()
        .expect("health timeout")
        .due_at_ns;
    let cleanup_due = timeout + 5_000_000_000;
    let mut controller = TestProcessController::default();

    supervisor
        .process_due_health_check_timeouts(&mut controller, timeout)
        .expect("health timeout");
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/app/health", true);

    supervisor
        .process_due_cgroup_cleanups(&mut controller, cleanup_due)
        .expect("cgroup cleanup")
        .expect("a cleanup deadline was due");

    let status = supervisor.service_status("app").expect("status");
    assert!(
        status.warnings.iter().any(|warning| {
            warning.path == "/sys/fs/cgroup/peinit/app/health"
                && warning.warning_type == ServiceStatusWarningType::Health
                && warning.detected_at_ns == cleanup_due
        }),
        "expected health leak warning, got {:?}",
        status.warnings,
    );
    assert_eq!(
        status.lifecycle_warnings,
        vec![
            "service has leaked sub-cgroups from a previous generation -- indicates underlying I/O problem requiring investigation"
        ]
    );
    assert_eq!(
        controller.cgroup_populated_checks,
        vec![
            "/sys/fs/cgroup/peinit/app".to_string(),
            "/sys/fs/cgroup/peinit/app/health".to_string(),
        ],
    );
}
