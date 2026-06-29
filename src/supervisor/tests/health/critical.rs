use crate::shutdown::ShutdownKind;
use crate::supervisor::tests::TestProcessController;

use super::support::{
    FinalizerCall, RecordingFinalizer, critical_active_health_supervisor, launch_due_health_check,
};

#[test]
fn critical_health_timeout_budget_exhaustion_reboots_with_finalizer() {
    let (mut supervisor, first_due) = critical_active_health_supervisor();
    launch_due_health_check(&mut supervisor, first_due, 9000, 70);
    let timeout = supervisor
        .next_health_check_timeout()
        .expect("health timeout")
        .due_at_ns;
    let mut controller = TestProcessController::default();
    let mut finalizer = RecordingFinalizer::default();

    let timeouts = supervisor
        .process_due_health_check_timeouts_with_finalizer(
            &mut controller,
            Some(&mut finalizer),
            timeout,
        )
        .expect("health timeout");

    assert_eq!(timeouts.len(), 1);
    assert!(timeouts[0].terminal.critical_reboot.is_some());
    assert_eq!(
        finalizer.calls,
        vec![FinalizerCall::Sync, FinalizerCall::Reboot],
    );
    assert!(supervisor.shutdown().is_some());
}

#[test]
fn critical_health_terminal_budget_exhaustion_reboots_with_finalizer() {
    let (mut supervisor, first_due) = critical_active_health_supervisor();
    let launch = launch_due_health_check(&mut supervisor, first_due, 9000, 70);
    let mut controller = TestProcessController::default();
    let mut finalizer = RecordingFinalizer::default();

    let terminal = supervisor
        .complete_health_check_job_with_shutdown_finalizer(
            launch.launch.job_id,
            first_due + 100_000,
            1,
            &mut controller,
            &mut finalizer,
        )
        .expect("health terminal");

    assert!(terminal.critical_reboot.is_some());
    assert_eq!(
        finalizer.calls,
        vec![FinalizerCall::Sync, FinalizerCall::Reboot],
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").kind,
        ShutdownKind::Reboot,
    );
}
