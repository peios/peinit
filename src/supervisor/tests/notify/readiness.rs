use crate::execution::notify::{NotifyAppliedField, NotifyApplyError};
use crate::operation::{OperationState, operation_timeout_result};
use crate::service::ServiceDefinition;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::{Supervisor, SupervisorError, SupervisorSettings};

use super::super::{
    AUTHD_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, process, settings,
};
use super::{
    NOTIFY_NS, apply_notify, apply_notify_with_controller, datagram, notify_app_supervisor,
};

#[test]
fn ready_notification_satisfies_notify_readiness_and_releases_dependent() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.requires.push("authd".to_string());
    let mut authd = ServiceDefinition::simple_system_boot("authd", "/sbin/authd");
    authd.triggers.clear();

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, authd]);
    let mut clock = ScriptedClock::new([BOOT_NS, AUTHD_LAUNCH_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let authd_operation = boot.start_dispatches[0].ready.operation_id;

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    let launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch authd")
        .expect("authd launch dispatch");

    assert!(launch.start_dispatches.is_empty());
    assert_eq!(
        supervisor.service_status("authd").expect("authd").state,
        ServiceState::Starting,
    );

    let dispatch = apply_notify(
        &mut supervisor,
        datagram(4242, b"STATUS=Listening\nREADY=1"),
        NOTIFY_NS,
    )
    .expect("apply ready notification");

    assert_eq!(
        dispatch.notify.applied_fields,
        vec![
            NotifyAppliedField::Status {
                text: "Listening".to_string()
            },
            NotifyAppliedField::Ready,
        ],
    );
    assert_eq!(
        dispatch
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    assert_eq!(
        supervisor.service_status("authd").expect("authd").state,
        ServiceState::Active,
    );
    assert_eq!(
        supervisor
            .service_status("authd")
            .expect("authd")
            .status_text
            .as_deref(),
        Some("Listening"),
    );
    assert_eq!(
        supervisor
            .operation_status(authd_operation)
            .expect("authd operation")
            .state,
        OperationState::Completed,
    );
    assert_eq!(
        supervisor
            .next_readiness_timeout()
            .expect("app timeout")
            .service,
        "app",
    );
}

#[test]
fn unauthenticated_notification_is_rejected_without_state_change() {
    let mut supervisor = notify_app_supervisor();
    let operation_id = supervisor
        .next_readiness_timeout()
        .expect("readiness timeout")
        .operation_id;

    let error = apply_notify(&mut supervisor, datagram(9999, b"READY=1"), NOTIFY_NS)
        .expect_err("reject unauthenticated notification");

    assert_eq!(
        error,
        SupervisorError::Notify(NotifyApplyError::UnauthenticatedSender { pid: 9999 }),
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("operation")
            .state,
        OperationState::Running,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").status_text,
        None,
    );
}

#[test]
fn pidfd_mismatch_rejects_authenticated_pid_without_state_change() {
    let mut supervisor = notify_app_supervisor();
    let mut controller = TestProcessController::default();
    controller.set_pidfd_match(50, 8000, false);
    let operation_id = supervisor
        .next_readiness_timeout()
        .expect("readiness timeout")
        .operation_id;

    let error = apply_notify_with_controller(
        &mut supervisor,
        datagram(8000, b"READY=1"),
        NOTIFY_NS,
        &mut controller,
    )
    .expect_err("reject pidfd mismatch");

    assert!(matches!(
        error,
        SupervisorError::Notify(NotifyApplyError::PidfdMismatch {
            pid: 8000,
            pidfd: 50,
            ..
        })
    ));
    assert_eq!(controller.pidfd_match_checks, vec![(50, 8000)]);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("operation")
            .state,
        OperationState::Running,
    );
}

/// §10.5 step 5: a datagram from a job whose activation generation is not the
/// service's current one is rejected as a previous incarnation's.
///
/// This state is unreachable in a running system and by design: `create.rs`
/// forbids a second live service-main job, so the previous incarnation is
/// always reaped before a transition to Starting bumps the generation — a live
/// process at a stale generation cannot exist. It is staged here by pushing the
/// current main job's generation back by one, which is exactly the skew the
/// check exists to catch, and the datagram is otherwise perfectly good: correct
/// pid, Running job, matching pidfd. Only the generation stops it.
#[test]
fn a_stale_activation_generation_is_rejected_without_state_change() {
    let mut supervisor = notify_app_supervisor();

    let job_id = supervisor
        .jobs()
        .current_service_main_job("app")
        .expect("app has a current main job");
    let current = supervisor.services().runtime("app").expect("runtime").generation;
    let stale = current.checked_sub(1).unwrap_or(current + 1);
    supervisor
        .jobs_mut()
        .record_mut(job_id)
        .expect("job record")
        .activation_generation = stale;

    let operation_id = supervisor
        .next_readiness_timeout()
        .expect("readiness timeout")
        .operation_id;

    let error = apply_notify(&mut supervisor, datagram(8000, b"READY=1"), NOTIFY_NS)
        .expect_err("reject a stale-generation notification");

    assert!(
        matches!(
            error,
            SupervisorError::Notify(NotifyApplyError::GenerationMismatch {
                job_generation,
                runtime_generation,
                ..
            }) if job_generation == stale && runtime_generation == current
        ),
        "the datagram is refused for its generation, got {error:?}",
    );
    // A READY=1 from a previous incarnation cannot mark the replacement ready:
    // the service stays Starting and its start operation stays Running.
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting,
    );
    assert_eq!(
        supervisor
            .operation_status(operation_id)
            .expect("operation")
            .state,
        OperationState::Running,
    );
}

#[test]
fn readiness_timeout_kills_service_cgroup_and_fails_start() {
    let mut supervisor = notify_app_supervisor();
    let deadline = supervisor
        .next_readiness_timeout()
        .expect("readiness timeout");
    let mut controller = TestProcessController::default();

    let timeout = supervisor
        .process_next_due_readiness_timeout(&mut controller, deadline.due_at_ns)
        .expect("process readiness timeout")
        .expect("timeout dispatch");

    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app".to_string()],
    );
    assert_eq!(
        timeout.timeout.killed_cgroup_id,
        "/sys/fs/cgroup/peinit/app",
    );
    let status = supervisor.service_status("app").expect("app");
    assert_eq!(status.state, ServiceState::Backoff);
    assert_eq!(status.cause, Some(TransitionCause::ReadinessTimeout));
    let operation = supervisor
        .operation_status(deadline.operation_id)
        .expect("operation");
    assert_eq!(operation.state, OperationState::Failed);
    let expected = operation_timeout_result("start timed out waiting for READY=1");
    assert_eq!(operation.error.as_deref(), Some(expected.as_str()));
    assert!(supervisor.next_readiness_timeout().is_none());
}
