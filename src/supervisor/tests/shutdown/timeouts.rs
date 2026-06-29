use std::collections::BTreeSet;

use crate::boundary::CgroupRemoveOutcome;
use crate::job::JobState;
use crate::service::runtime::ServiceState;
use crate::shutdown::{ShutdownFinalizationState, ShutdownKind};

use super::super::TestProcessController;
use super::SHUTDOWN_NS;
use super::fixture::{job_for, shutdown_fixture};

const STOP_TIMEOUT_NS: u64 = 10_000_000_000;
const POST_KILL_TIMEOUT_NS: u64 = 5_000_000_000;

#[test]
fn shutdown_global_deadline_uses_configured_timeout() {
    let mut supervisor = shutdown_fixture();
    supervisor.settings.shutdown.global_timeout_secs = 12;
    let mut controller = TestProcessController::default();

    let dispatch = supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    assert_eq!(
        dispatch.runtime.global_deadline_ns,
        SHUTDOWN_NS + 12_000_000_000,
    );
}

#[test]
fn per_service_shutdown_timeout_kills_cgroup_then_abandons_after_post_kill_timeout() {
    let mut supervisor = shutdown_fixture();
    let draining_job = job_for(&supervisor, "draining");
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    supervisor
        .complete_shutdown_job(draining_job, SHUTDOWN_NS + 1, 0, &mut controller)
        .expect("complete draining");

    let stop_timeout = supervisor
        .process_due_shutdown_timeouts(&mut controller, SHUTDOWN_NS + STOP_TIMEOUT_NS)
        .expect("process stop timeout")
        .expect("timeout dispatch");

    assert!(!stop_timeout.global_timeout);
    assert_eq!(stop_timeout.cgroup_kills.len(), 1);
    assert_eq!(stop_timeout.cgroup_kills[0].service, "app");
    assert_eq!(
        stop_timeout.cgroup_kills[0].cgroup_id,
        "/sys/fs/cgroup/peinit/app",
    );
    assert!(stop_timeout.abandoned.is_empty());
    assert!(stop_timeout.next_wave.is_empty());
    assert_eq!(
        stop_timeout.finalization,
        ShutdownFinalizationState::WaitingForServices,
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").post_kill_deadlines[0].due_at_ns,
        SHUTDOWN_NS + STOP_TIMEOUT_NS + POST_KILL_TIMEOUT_NS,
    );

    let post_kill_timeout = supervisor
        .process_due_shutdown_timeouts(
            &mut controller,
            SHUTDOWN_NS + STOP_TIMEOUT_NS + POST_KILL_TIMEOUT_NS,
        )
        .expect("process post-kill timeout")
        .expect("post-kill dispatch");

    assert_eq!(post_kill_timeout.abandoned.len(), 1);
    assert_eq!(post_kill_timeout.job_events.len(), 1);
    assert_eq!(
        post_kill_timeout.job_events[0].service.as_deref(),
        Some("app")
    );
    assert_eq!(post_kill_timeout.job_events[0].state, JobState::Abandoned);
    assert_eq!(
        post_kill_timeout.job_events[0].failure_cause.as_deref(),
        Some("process survived SIGKILL"),
    );
    assert!(supervisor.jobs().current_service_main_job("app").is_none());
    assert_eq!(
        controller.cgroup_populated_checks,
        vec!["/sys/fs/cgroup/peinit/app".to_string()],
    );
    assert_eq!(post_kill_timeout.abandoned[0].service, "app");
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Abandoned,
    );
    assert_eq!(post_kill_timeout.next_wave.len(), 1);
    assert_eq!(post_kill_timeout.next_wave[0].service, "db");
    assert_eq!(
        post_kill_timeout.finalization,
        ShutdownFinalizationState::WaitingForServices,
    );
}

#[test]
fn post_kill_timeout_with_empty_cgroup_cleans_tree_and_finishes_service() {
    let mut supervisor = shutdown_fixture();
    let draining_job = job_for(&supervisor, "draining");
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    supervisor
        .complete_shutdown_job(draining_job, SHUTDOWN_NS + 1, 0, &mut controller)
        .expect("complete draining");

    supervisor
        .process_due_shutdown_timeouts(&mut controller, SHUTDOWN_NS + STOP_TIMEOUT_NS)
        .expect("process stop timeout")
        .expect("timeout dispatch");
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/app", false);
    controller.set_cgroup_remove_result(
        "/sys/fs/cgroup/peinit/app/hooks",
        CgroupRemoveOutcome::Missing,
    );

    let post_kill_timeout = supervisor
        .process_due_shutdown_timeouts(
            &mut controller,
            SHUTDOWN_NS + STOP_TIMEOUT_NS + POST_KILL_TIMEOUT_NS,
        )
        .expect("process post-kill timeout")
        .expect("post-kill dispatch");

    assert!(post_kill_timeout.abandoned.is_empty());
    assert_eq!(post_kill_timeout.job_events.len(), 1);
    assert_eq!(
        post_kill_timeout.job_events[0].service.as_deref(),
        Some("app")
    );
    assert_eq!(post_kill_timeout.job_events[0].state, JobState::Failed);
    assert_eq!(
        post_kill_timeout.job_events[0].failure_cause.as_deref(),
        Some("service cgroup emptied after SIGKILL before exit status was observed"),
    );
    assert!(supervisor.jobs().current_service_main_job("app").is_none());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );
    assert_eq!(
        controller.cgroup_populated_checks,
        vec!["/sys/fs/cgroup/peinit/app".to_string()],
    );
    assert_eq!(
        controller.cgroup_removes,
        vec![
            "/sys/fs/cgroup/peinit/app/main".to_string(),
            "/sys/fs/cgroup/peinit/app/hooks".to_string(),
            "/sys/fs/cgroup/peinit/app/health".to_string(),
            "/sys/fs/cgroup/peinit/app".to_string(),
        ],
    );
    assert_eq!(post_kill_timeout.next_wave.len(), 1);
    assert_eq!(post_kill_timeout.next_wave[0].service, "db");
}

#[test]
fn global_shutdown_timeout_kills_all_remaining_services_and_readies_after_abandonment() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Halt, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    let global_due = supervisor.shutdown().expect("shutdown").global_deadline_ns;

    let global_timeout = supervisor
        .process_due_shutdown_timeouts(&mut controller, global_due)
        .expect("process global timeout")
        .expect("global timeout dispatch");

    assert!(global_timeout.global_timeout);
    assert_eq!(
        killed_services(&global_timeout.cgroup_kills),
        BTreeSet::from(["app", "db", "draining"]),
    );
    assert!(global_timeout.abandoned.is_empty());
    assert_eq!(
        global_timeout.finalization,
        ShutdownFinalizationState::WaitingForServices,
    );

    let post_kill_timeout = supervisor
        .process_due_shutdown_timeouts(&mut controller, global_due + POST_KILL_TIMEOUT_NS)
        .expect("process post-kill timeout")
        .expect("post-kill dispatch");

    assert_eq!(
        abandoned_services(&post_kill_timeout.abandoned),
        BTreeSet::from(["app", "db", "draining"]),
    );
    assert_eq!(
        post_kill_timeout.finalization,
        ShutdownFinalizationState::Ready
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").finalization,
        ShutdownFinalizationState::Ready,
    );
}

fn killed_services(
    kills: &[crate::supervisor::SupervisorShutdownCgroupKillDispatch],
) -> BTreeSet<&str> {
    kills.iter().map(|kill| kill.service.as_str()).collect()
}

fn abandoned_services(
    abandoned: &[crate::supervisor::SupervisorShutdownAbandonedDispatch],
) -> BTreeSet<&str> {
    abandoned
        .iter()
        .map(|abandoned| abandoned.service.as_str())
        .collect()
}
