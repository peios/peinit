use crate::ids::OperationId;
use crate::operation::store::OperationEventDetail;
use crate::operation::{OperationSource, OperationState, OperationType};
use crate::shutdown::ShutdownKind;
use crate::supervisor::Supervisor;

use super::super::TestProcessController;
use super::SHUTDOWN_NS;
use super::fixture::{
    LATER_WAVE_STOP_DEADLINE_NS, job_for, later_wave_pair_fixture, later_wave_stopping_fixture,
    shutdown_fixture,
};

fn observation(supervisor: &Supervisor, service: &str) -> OperationId {
    supervisor
        .operations
        .active_for_service(service)
        .into_iter()
        .find(|id| supervisor.operations.get(*id).unwrap().source == OperationSource::Shutdown)
        .expect("shutdown creates its own observable Stop record")
}

#[test]
fn shutdown_records_every_wave_but_only_starts_the_eligible_wave() {
    let mut supervisor = later_wave_pair_fixture();
    let mut controller = TestProcessController::default();
    let begun = supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    let requests: Vec<_> = begun
        .operation_events
        .iter()
        .filter(|event| matches!(event.detail, OperationEventDetail::Requested))
        .collect();
    assert_eq!(
        requests
            .iter()
            .map(|e| e.service.as_str())
            .collect::<Vec<_>>(),
        ["front", "left", "right"]
    );
    assert!(
        requests
            .iter()
            .all(|e| e.source == OperationSource::Shutdown
                && e.operation_type == OperationType::Stop
                && e.caller.is_none())
    );
    let front = observation(&supervisor, "front");
    let left = observation(&supervisor, "left");
    let right = observation(&supervisor, "right");
    assert_eq!(
        supervisor.operation_status(front).unwrap().state,
        OperationState::Running
    );
    assert_eq!(
        supervisor.operation_status(left).unwrap().state,
        OperationState::Pending
    );
    assert_eq!(
        supervisor.operation_status(right).unwrap().state,
        OperationState::Pending
    );
    assert_eq!(controller.signals.len(), 1);
    assert!(!supervisor.has_ready_queued_operations());
    let done = supervisor
        .complete_shutdown_job(
            job_for(&supervisor, "front"),
            SHUTDOWN_NS + 1,
            0,
            &mut controller,
        )
        .unwrap();
    assert_eq!(
        done.operation_events
            .iter()
            .map(|e| (e.service.as_str(), e.state))
            .collect::<Vec<_>>(),
        [
            ("front", OperationState::Completed),
            ("left", OperationState::Running),
            ("right", OperationState::Running)
        ]
    );
    assert_eq!(
        supervisor.operation_status(front).unwrap().state,
        OperationState::Completed
    );
}

#[test]
fn pending_shutdown_expiry_never_signals_early_or_cancels_the_later_wave() {
    let mut supervisor = later_wave_pair_fixture();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    let left = observation(&supervisor, "left");
    let right = observation(&supervisor, "right");
    let expiry = SHUTDOWN_NS + 10_000_000_000;
    let maintenance = supervisor
        .process_due_operation_maintenance(expiry)
        .unwrap();
    assert_eq!(maintenance.operation_timeouts.len(), 2);
    assert_eq!(
        supervisor.operation_status(left).unwrap().state,
        OperationState::Failed
    );
    assert_eq!(controller.signals.len(), 1);
    assert!(controller.cgroup_kills.is_empty());
    assert!(!supervisor.has_ready_queued_operations());
    let dispatch = supervisor
        .complete_shutdown_job(
            job_for(&supervisor, "front"),
            expiry + 1,
            0,
            &mut controller,
        )
        .unwrap();
    assert_eq!(
        controller.signals.len(),
        3,
        "wave still stops both participants after their observations expire"
    );
    assert_eq!(
        dispatch.operation_events.len(),
        1,
        "only front completes; expired observations do not resurrect"
    );
    assert_eq!(
        supervisor.operation_status(right).unwrap().state,
        OperationState::Failed
    );
    assert!(
        dispatch
            .next_wave
            .iter()
            .all(|stop| stop.deadline.as_ref().unwrap().due_at_ns == expiry + 1 + 10_000_000_000)
    );
}

#[test]
fn purged_expired_observations_do_not_block_later_waves() {
    let mut supervisor = later_wave_pair_fixture();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    let left = observation(&supervisor, "left");
    supervisor
        .process_due_operation_maintenance(SHUTDOWN_NS + 10_000_000_000)
        .unwrap();
    let later = SHUTDOWN_NS + 70_000_000_001;
    supervisor.process_due_operation_maintenance(later).unwrap();
    assert!(supervisor.operations.get(left).is_none());
    let done = supervisor
        .complete_shutdown_job(job_for(&supervisor, "front"), later + 1, 0, &mut controller)
        .unwrap();
    assert_eq!(done.next_wave.len(), 2);
    assert_eq!(controller.signals.len(), 3);
}

#[test]
fn shutdown_observation_preserves_an_existing_admin_stop_and_its_budget() {
    let mut supervisor = later_wave_stopping_fixture();
    supervisor
        .operations
        .adopt_service_security(&supervisor.services);
    let existing = supervisor
        .operations
        .current_for_service("back")
        .unwrap()
        .clone();
    let old_deadline = supervisor
        .control
        .stop_timeout(existing.id)
        .unwrap()
        .clone();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    let back = observation(&supervisor, "back");
    assert_ne!(back, existing.id);
    assert_eq!(supervisor.operations.get(existing.id), Some(&existing));
    assert_eq!(
        supervisor.control.stop_timeout(existing.id),
        Some(old_deadline)
    );
    let done = supervisor
        .complete_shutdown_job(
            job_for(&supervisor, "front"),
            SHUTDOWN_NS + 1,
            0,
            &mut controller,
        )
        .unwrap();
    let retained = done
        .next_wave
        .iter()
        .find(|stop| stop.service == "back")
        .unwrap()
        .deadline
        .as_ref()
        .unwrap();
    assert_eq!(retained.due_at_ns, LATER_WAVE_STOP_DEADLINE_NS);
    assert_eq!(retained.operation_id, Some(existing.id));
    assert_eq!(
        supervisor.operation_status(back).unwrap().state,
        OperationState::Running
    );
    assert!(
        controller
            .signals
            .iter()
            .all(|signal| signal.target.service != "back")
    );
}

#[test]
fn a_later_wave_exit_settles_its_pending_observation_without_starting_it() {
    let mut supervisor = later_wave_pair_fixture();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    let left = observation(&supervisor, "left");
    let done = supervisor
        .complete_shutdown_job(
            job_for(&supervisor, "left"),
            SHUTDOWN_NS + 1,
            0,
            &mut controller,
        )
        .unwrap();
    assert_eq!(done.operation_events.len(), 1);
    assert_eq!(done.operation_events[0].state, OperationState::Completed);
    assert_eq!(supervisor.operations.get(left).unwrap().started_at_ns, None);
    assert_eq!(controller.signals.len(), 1);
}

#[test]
fn a_later_wave_crash_fails_its_pending_observation() {
    let mut supervisor = later_wave_pair_fixture();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    let done = supervisor
        .fail_running_shutdown_job(
            job_for(&supervisor, "left"),
            SHUTDOWN_NS + 1,
            Some(crate::job::JobExit::Signal(libc::SIGSEGV)),
            "crashed",
            &mut controller,
        )
        .unwrap();
    assert_eq!(done.operation_events.len(), 1);
    assert_eq!(done.operation_events[0].state, OperationState::Failed);
    assert_eq!(controller.signals.len(), 1);
}

#[test]
fn stop_timeout_records_one_failure_even_when_the_service_later_exits() {
    let mut supervisor = later_wave_pair_fixture();
    let mut controller = TestProcessController::default();
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/front", true);
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    let front = observation(&supervisor, "front");
    let now = SHUTDOWN_NS + 10_000_000_000;
    let timeout = supervisor
        .process_due_shutdown_timeouts(&mut controller, now)
        .unwrap()
        .unwrap();
    assert_eq!(timeout.operation_events.len(), 1);
    assert_eq!(timeout.operation_events[0].operation_id, front);
    assert_eq!(timeout.operation_events[0].state, OperationState::Failed);
    let done = supervisor
        .complete_shutdown_job(job_for(&supervisor, "front"), now + 1, 0, &mut controller)
        .unwrap();
    assert!(
        done.operation_events
            .iter()
            .all(|event| event.operation_id != front)
    );
    assert_eq!(
        supervisor.operation_status(front).unwrap().state,
        OperationState::Failed
    );
}

#[test]
fn global_timeout_fails_running_and_pending_shutdown_observations() {
    let mut supervisor = later_wave_pair_fixture();
    supervisor.settings.shutdown.global_timeout_secs = 1;
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    let timeout = supervisor
        .process_due_shutdown_timeouts(&mut controller, SHUTDOWN_NS + 1_000_000_000)
        .unwrap()
        .unwrap();
    assert!(timeout.global_timeout);
    assert_eq!(timeout.operation_events.len(), 3);
    assert!(
        timeout
            .operation_events
            .iter()
            .all(|e| e.state == OperationState::Failed)
    );
    assert_eq!(controller.signals.len(), 1);
    assert_eq!(controller.cgroup_kills.len(), 3);
}

#[test]
fn forced_reboot_terminates_graceful_shutdown_observations() {
    let mut supervisor = later_wave_pair_fixture();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    let forced = supervisor
        .force_reboot_shutdown(&mut controller, None, SHUTDOWN_NS + 1)
        .unwrap();
    assert_eq!(forced.operation_events.len(), 3);
    assert!(
        forced
            .operation_events
            .iter()
            .all(|e| e.state == OperationState::Failed)
    );
}

#[test]
fn shutdown_records_only_stop_wave_participants() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let begun = supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    assert!(
        begun
            .operation_events
            .iter()
            .all(|event| ["app", "db", "draining"].contains(&event.service.as_str()))
    );
    assert_eq!(
        begun
            .operation_events
            .iter()
            .filter(|e| matches!(e.detail, OperationEventDetail::Requested))
            .count(),
        3
    );
    assert_eq!(
        begun.startup_operation_events.len(),
        1,
        "startup cancellation retains its own event"
    );
}

#[test]
fn running_observations_follow_the_wave_deadline_and_its_extensions() {
    let mut supervisor = later_wave_pair_fixture();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .unwrap();
    let left = observation(&supervisor, "left");
    supervisor
        .complete_shutdown_job(
            job_for(&supervisor, "front"),
            SHUTDOWN_NS + 9_000_000_000,
            0,
            &mut controller,
        )
        .unwrap();
    // Its request is eleven seconds old, but this stop phase is only two.
    assert!(!supervisor.operation_timeout_expired(left, SHUTDOWN_NS + 11_000_000_000));
    assert!(supervisor.operation_timeout_expired(left, SHUTDOWN_NS + 19_000_000_000));
    supervisor
        .shutdown
        .as_mut()
        .unwrap()
        .stop_deadlines
        .iter_mut()
        .find(|deadline| deadline.service == "left")
        .unwrap()
        .due_at_ns += 10_000_000_000;
    assert!(!supervisor.operation_timeout_expired(left, SHUTDOWN_NS + 19_000_000_000));
    assert!(supervisor.operation_timeout_expired(left, SHUTDOWN_NS + 29_000_000_000));
}
