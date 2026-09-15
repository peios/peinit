use crate::boundary::ProcessSignal;
use crate::job::JobExit;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::shutdown::{ShutdownFinalizationState, ShutdownKind};

use super::super::TestProcessController;
use super::SHUTDOWN_NS;
use super::fixture::{job_for, later_wave_pair_fixture, shutdown_fixture};

// PEI-1086. The plan is fixed when the shutdown begins; a participant of a
// later wave can crash while an earlier wave is still draining. When its wave
// began, begin_stop_wave asked for its process, got MissingRunningService,
// and the `?` ended PID 1's runtime loop mid-shutdown. A participant already
// done is skipped instead — the same test wave_complete applies — and the
// rest of the wave proceeds.
#[test]
fn a_participant_that_failed_before_its_wave_is_skipped_when_the_wave_begins() {
    let mut supervisor = later_wave_pair_fixture();
    let front_job = job_for(&supervisor, "front");
    let left_job = job_for(&supervisor, "left");
    let right_job = job_for(&supervisor, "right");
    let mut controller = TestProcessController::default();
    let begun = supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    assert_eq!(
        begun.runtime.plan.stop_waves[1]
            .services
            .iter()
            .map(|participant| participant.service.as_str())
            .collect::<Vec<_>>(),
        vec!["left", "right"],
        "left and right share wave 1",
    );

    // right crashes while wave 0 is still draining.
    let crashed = supervisor
        .fail_running_shutdown_job(
            right_job,
            SHUTDOWN_NS + 1,
            Some(JobExit::Signal(libc::SIGSEGV)),
            "segfault",
            &mut controller,
        )
        .expect("right crashes");
    assert!(crashed.next_wave.is_empty(), "wave 0 is still open");
    let right = supervisor.service_status("right").expect("right");
    assert_eq!(right.state, ServiceState::Failed);
    assert_eq!(right.cause, Some(TransitionCause::ProcessCrash));

    let front_done = supervisor
        .complete_shutdown_job(front_job, SHUTDOWN_NS + 2, 0, &mut controller)
        .expect("a participant already Failed must not end the shutdown when its wave begins");

    assert_eq!(
        front_done
            .next_wave
            .iter()
            .map(|stop| (stop.service.as_str(), stop.signal.clone()))
            .collect::<Vec<_>>(),
        vec![("left", Some(ProcessSignal::Sigterm))],
        "only the participant still running is stopped, and nothing is recorded for right",
    );
    assert_eq!(
        supervisor
            .shutdown()
            .expect("shutdown")
            .stop_deadlines
            .iter()
            .map(|deadline| deadline.service.as_str())
            .collect::<Vec<_>>(),
        vec!["left"],
    );
    assert_eq!(
        supervisor.service_status("left").expect("left").state,
        ServiceState::Stopping,
    );
    assert_eq!(
        front_done.finalization,
        ShutdownFinalizationState::WaitingForServices,
    );

    let left_done = supervisor
        .complete_shutdown_job(left_job, SHUTDOWN_NS + 3, 0, &mut controller)
        .expect("complete left");
    assert_eq!(left_done.finalization, ShutdownFinalizationState::Ready);
}

#[test]
fn shutdown_terminal_events_advance_reverse_dependency_waves() {
    let mut supervisor = shutdown_fixture();
    let app_job = job_for(&supervisor, "app");
    let draining_job = job_for(&supervisor, "draining");
    let db_job = job_for(&supervisor, "db");
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    let app_dispatch = supervisor
        .complete_shutdown_job(app_job, SHUTDOWN_NS + 1, 0, &mut controller)
        .expect("complete app");

    assert!(app_dispatch.next_wave.is_empty());
    assert_eq!(
        app_dispatch.finalization,
        ShutdownFinalizationState::WaitingForServices,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );

    let draining_dispatch = supervisor
        .complete_shutdown_job(draining_job, SHUTDOWN_NS + 2, 0, &mut controller)
        .expect("complete draining");

    assert_eq!(draining_dispatch.next_wave.len(), 1);
    assert_eq!(draining_dispatch.next_wave[0].service, "db");
    assert_eq!(
        draining_dispatch.next_wave[0].signal,
        Some(ProcessSignal::Sigterm),
    );
    assert_eq!(
        draining_dispatch.finalization,
        ShutdownFinalizationState::WaitingForServices,
    );
    assert_eq!(
        supervisor.service_status("db").expect("db").state,
        ServiceState::Stopping,
    );

    let db_dispatch = supervisor
        .complete_shutdown_job(db_job, SHUTDOWN_NS + 3, 0, &mut controller)
        .expect("complete db");

    assert!(db_dispatch.next_wave.is_empty());
    assert_eq!(db_dispatch.finalization, ShutdownFinalizationState::Ready);
    assert_eq!(
        supervisor.shutdown().expect("shutdown").finalization,
        ShutdownFinalizationState::Ready,
    );
}
