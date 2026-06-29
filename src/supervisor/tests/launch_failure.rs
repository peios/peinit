use crate::boundary::{BoundaryError, ProcessLaunchError, ProcessPreExecError, ProcessPreExecStep};
use crate::job::{JobEventDetail, JobState};
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::cgroup_cleanup::CgroupCleanupKind;
use crate::supervisor::{Supervisor, SupervisorServiceLaunchDispatch, SupervisorSettings};

use super::{
    AUTHD_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, settings,
};

#[test]
fn parent_setup_launch_failure_marks_start_failed_and_service_backoff() {
    let mut supervisor = boot_single_service("authd");
    let job_id = supervisor.pending_launch_jobs()[0];
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::results(vec![Err(BoundaryError::ProcessLaunch(
        ProcessLaunchError::parent_setup("pipe2 failed: EMFILE"),
    ))]);
    let mut clock = ScriptedClock::new([AUTHD_LAUNCH_NS]);

    let dispatch = supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch failure is handled")
        .expect("launch failure dispatch");
    let SupervisorServiceLaunchDispatch::Failed(dispatch) = dispatch else {
        panic!("expected failed launch dispatch");
    };

    assert_eq!(dispatch.job_event.job_id, job_id);
    assert_eq!(dispatch.job_event.state, JobState::Failed);
    assert_eq!(
        dispatch.failure.service_transitions[0].event.cause,
        TransitionCause::ParentSetupFailure
    );
    assert_eq!(
        dispatch.failure.operation_events[0].state,
        crate::operation::OperationState::Failed
    );
    assert_failure_cause(
        &dispatch.job_event,
        "ParentSetupFailure: pipe2 failed: EMFILE",
    );
    assert_service_backoff(&supervisor, "authd", TransitionCause::ParentSetupFailure);
    let cleanup = supervisor
        .next_cgroup_cleanup_deadline()
        .expect("cleanup deadline");
    assert_eq!(cleanup.service, "authd");
    assert_eq!(cleanup.cgroup_id, "/sys/fs/cgroup/peinit/authd");
    assert_eq!(cleanup.kind, CgroupCleanupKind::ServiceTree);
    assert_eq!(cleanup.due_at_ns, AUTHD_LAUNCH_NS);
    assert!(supervisor.pending_launch_jobs().is_empty());

    let mut controller = TestProcessController::default();
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/authd", false);
    assert!(
        supervisor
            .process_due_cgroup_cleanups(&mut controller, AUTHD_LAUNCH_NS)
            .expect("process cleanup")
    );
    assert_eq!(
        controller.cgroup_populated_checks,
        vec!["/sys/fs/cgroup/peinit/authd"]
    );
    assert_eq!(
        controller.cgroup_removes,
        vec![
            "/sys/fs/cgroup/peinit/authd/main",
            "/sys/fs/cgroup/peinit/authd/hooks",
            "/sys/fs/cgroup/peinit/authd/health",
            "/sys/fs/cgroup/peinit/authd",
        ]
    );
}

#[test]
fn pre_exec_launch_failure_marks_start_failed_and_service_backoff() {
    let mut supervisor = boot_single_service("authd");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::results(vec![Err(BoundaryError::ProcessLaunch(
        ProcessLaunchError::pre_exec(
            ProcessPreExecError {
                step: ProcessPreExecStep::SetRlimits,
                errno: libc::EINVAL,
            },
            Vec::new(),
        ),
    ))]);
    let mut clock = ScriptedClock::new([AUTHD_LAUNCH_NS]);

    let dispatch = supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch failure is handled")
        .expect("launch failure dispatch");
    let SupervisorServiceLaunchDispatch::Failed(dispatch) = dispatch else {
        panic!("expected failed launch dispatch");
    };

    assert_eq!(
        dispatch.failure.service_transitions[0].event.cause,
        TransitionCause::PreExecFailure
    );
    assert_failure_cause(
        &dispatch.job_event,
        "PreExecFailure: set-rlimits failed with errno 22",
    );
    assert_service_backoff(&supervisor, "authd", TransitionCause::PreExecFailure);
}

fn boot_single_service(service: &str) -> Supervisor {
    let definition = alive_service(service);
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![definition]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);
    supervisor
}

fn assert_failure_cause(event: &crate::job::JobEvent, expected: &str) {
    let JobEventDetail::Ended { failure_cause, .. } = &event.detail else {
        panic!("expected terminal job event");
    };
    assert_eq!(failure_cause.as_deref(), Some(expected));
}

fn assert_service_backoff(supervisor: &Supervisor, service: &str, cause: TransitionCause) {
    let status = supervisor.service_status(service).expect("service status");
    assert_eq!(status.state, ServiceState::Backoff);
    assert!(status.current_job.is_none());
    let runtime = supervisor.services().runtime(service).expect("runtime");
    assert_eq!(runtime.cause, Some(cause));
    assert_eq!(
        runtime.restart_backoff_until_ns,
        Some(AUTHD_LAUNCH_NS + 1_000_000_000),
    );
}
