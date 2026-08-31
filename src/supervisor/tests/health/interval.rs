use crate::boundary::BoundaryError;
use crate::job::{JobEventDetail, JobType};
use crate::service::runtime::{ServiceHealthStatus, ServiceState};
use crate::supervisor::tests::{
    ScriptedClock, TestProcessLauncher, TestTokenProvider, process,
};
use crate::supervisor::{
    SupervisorHealthCheckIntervalAction, SupervisorHealthCheckLaunchResult,
    SupervisorHealthCheckOutcome,
};

use super::support::{
    HEALTH_TIMEOUT_SECS, active_health_supervisor, launch_due_health_check,
    launch_next_health_check,
};

#[test]
fn active_service_with_health_check_schedules_and_launches_probe() {
    let (mut supervisor, first_due) = active_health_supervisor(3);

    let interval = supervisor
        .process_due_health_check_intervals(first_due)
        .expect("health interval");

    assert_eq!(interval.len(), 1);
    let SupervisorHealthCheckIntervalAction::Created { job_event } = &interval[0].action else {
        panic!("expected created health job");
    };
    assert_eq!(job_event.job_type, JobType::HealthCheck);
    assert_eq!(job_event.service.as_deref(), Some("app"));
    assert_eq!(
        supervisor.pending_health_check_launch_jobs(),
        vec![job_event.job_id]
    );

    let launch = launch_next_health_check(&mut supervisor, first_due + 10_000, 9000, 70);
    let JobEventDetail::Started { cgroup_id, .. } = &launch.launch.job_event.detail else {
        panic!("expected started health job");
    };
    assert_eq!(cgroup_id, "/sys/fs/cgroup/peinit/app/health");
    assert_eq!(
        supervisor
            .next_health_check_timeout()
            .expect("health timeout")
            .due_at_ns,
        first_due + 10_000 + HEALTH_TIMEOUT_SECS * 1_000_000_000,
    );
    assert_eq!(
        supervisor.service_status("app").expect("status").health,
        Some(ServiceHealthStatus::Unknown),
    );
}

#[test]
fn health_check_launch_uses_health_timeout_for_setup_handshake() {
    let (mut supervisor, first_due) = active_health_supervisor(3);
    supervisor
        .process_due_health_check_intervals(first_due)
        .expect("health interval");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(9000, 70)]);
    let mut clock = ScriptedClock::new([first_due + 10_000]);
    supervisor
        .launch_next_pending_health_check_job(
            &mut tokens,
            &mut launcher,
            &mut clock,
        )
        .expect("launch health")
        .expect("health launch result");

    assert_eq!(launcher.observed_setup_timeouts, vec![HEALTH_TIMEOUT_SECS]);
}

#[test]
// PEI-367. A launch failure was counted as a health check failure and
// escalated immediately. With HealthCheckRetries=1 — a reasonable setting for
// a probe an operator trusts — one transient authd unavailability was enough
// to restart the service.
//
// The distinction being collapsed is between "the probe ran and said the
// service is unhealthy" and "the probe could not be run". Only the first is
// evidence about the service.
fn a_health_check_that_cannot_be_launched_does_not_count_against_the_service() {
    let (mut supervisor, first_due) = active_health_supervisor(1);
    supervisor
        .process_due_health_check_intervals(first_due)
        .expect("health interval");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::results(vec![Err(BoundaryError::Process(
        "clone3 EAGAIN".to_string(),
    ))]);
    let mut clock = ScriptedClock::new([first_due + 10_000]);

    let result = supervisor
        .launch_next_pending_health_check_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch health")
        .expect("health launch result");
    let SupervisorHealthCheckLaunchResult::Failed(dispatch) = result else {
        panic!("expected health launch failure");
    };

    assert_eq!(
        dispatch.terminal.outcome,
        SupervisorHealthCheckOutcome::NotLaunched,
    );
    // The service is untouched: still running, still Active, no restart.
    assert!(dispatch.terminal.service_job_event.is_none());
    assert!(dispatch.terminal.service_transitions.is_empty());
    assert_eq!(
        supervisor.service_status("app").expect("status").state,
        ServiceState::Active,
    );
    assert!(supervisor.jobs().current_service_main_job("app").is_some());
}

#[test]
fn interval_skips_when_previous_health_check_is_still_running() {
    let (mut supervisor, first_due) = active_health_supervisor(3);
    launch_due_health_check(&mut supervisor, first_due, 9000, 70);
    let second_due = first_due + super::support::HEALTH_INTERVAL_SECS * 1_000_000_000;

    let interval = supervisor
        .process_due_health_check_intervals(second_due)
        .expect("second interval");

    assert_eq!(interval.len(), 1);
    assert!(matches!(
        interval[0].action,
        SupervisorHealthCheckIntervalAction::SkippedOverlap { job_id: Some(_) }
    ));
    assert!(supervisor.pending_health_check_launch_jobs().is_empty());
    assert_eq!(
        supervisor.service_status("app").expect("status").state,
        ServiceState::Active,
    );
}
