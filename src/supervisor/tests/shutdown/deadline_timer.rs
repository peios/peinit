use crate::boundary::{BoundaryError, ShutdownDeadlineTimer};
use crate::shutdown::{ShutdownDeadlineKind, ShutdownError, ShutdownKind};
use crate::supervisor::{SupervisorError, SupervisorShutdownDeadlineTimerTurn};

use super::super::TestProcessController;
use super::SHUTDOWN_NS;
use super::fixture::{DRAINING_STOP_DEADLINE_NS, drive_shutdown_to_ready, shutdown_fixture};

#[test]
fn shutdown_deadline_timer_is_disarmed_when_no_shutdown_deadline_exists() {
    let supervisor = shutdown_fixture();
    let mut timer = FakeShutdownDeadlineTimer::default();

    let turn = supervisor
        .sync_shutdown_deadline_timer(&mut timer)
        .expect("sync timer");

    assert_eq!(turn, SupervisorShutdownDeadlineTimerTurn::Disarmed);
    assert_eq!(timer.calls, vec![TimerCall::Disarm]);
}

#[test]
fn shutdown_deadline_timer_arms_earliest_shutdown_deadline() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut timer = FakeShutdownDeadlineTimer::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    let turn = supervisor
        .sync_shutdown_deadline_timer(&mut timer)
        .expect("sync timer");

    let SupervisorShutdownDeadlineTimerTurn::Armed { deadline } = turn else {
        panic!("expected armed timer");
    };
    assert_eq!(deadline.due_at_ns, DRAINING_STOP_DEADLINE_NS);
    assert_eq!(
        deadline.kind,
        ShutdownDeadlineKind::StopTimeout {
            service: "draining".to_string(),
        },
    );
    assert_eq!(timer.calls, vec![TimerCall::Arm(DRAINING_STOP_DEADLINE_NS)]);
}

#[test]
fn shutdown_deadline_timer_disarms_after_shutdown_is_ready_to_finalize() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut timer = FakeShutdownDeadlineTimer::default();

    drive_shutdown_to_ready(&mut supervisor, ShutdownKind::Reboot, &mut controller);

    let turn = supervisor
        .sync_shutdown_deadline_timer(&mut timer)
        .expect("sync timer");

    assert_eq!(turn, SupervisorShutdownDeadlineTimerTurn::Disarmed);
    assert_eq!(timer.calls, vec![TimerCall::Disarm]);
}

#[test]
fn shutdown_deadline_timer_boundary_failure_is_reported_as_shutdown_error() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut timer = FakeShutdownDeadlineTimer {
        arm_error: Some("arm failed"),
        ..FakeShutdownDeadlineTimer::default()
    };
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    let err = supervisor
        .sync_shutdown_deadline_timer(&mut timer)
        .expect_err("timer failure");

    assert!(matches!(
        err,
        SupervisorError::Shutdown(ShutdownError::Boundary(BoundaryError::Timer(message)))
            if message == "arm failed"
    ));
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TimerCall {
    Arm(u64),
    Disarm,
}

#[derive(Debug, Default)]
struct FakeShutdownDeadlineTimer {
    calls: Vec<TimerCall>,
    arm_error: Option<&'static str>,
    disarm_error: Option<&'static str>,
}

impl ShutdownDeadlineTimer for FakeShutdownDeadlineTimer {
    fn arm_absolute_ns(&mut self, deadline_ns: u64) -> Result<(), BoundaryError> {
        self.calls.push(TimerCall::Arm(deadline_ns));
        match self.arm_error {
            Some(message) => Err(BoundaryError::Timer(message.to_string())),
            None => Ok(()),
        }
    }

    fn disarm(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(TimerCall::Disarm);
        match self.disarm_error {
            Some(message) => Err(BoundaryError::Timer(message.to_string())),
            None => Ok(()),
        }
    }
}
