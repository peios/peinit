use crate::boundary::{BoundaryError, LinuxSignalFdRead, ShutdownFinalizer};
use crate::shutdown::{ShutdownKind, ShutdownSignal};
use crate::supervisor::{SupervisorPid1SignalFdTurn, SupervisorShutdownSignalAction};

use super::super::{ScriptedClock, TestProcessController};
use super::SHUTDOWN_NS;
use super::fixture::shutdown_fixture;

#[test]
fn sigterm_begins_graceful_poweroff_shutdown() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut finalizer = SignalFinalizer::default();

    let dispatch = supervisor
        .handle_shutdown_signal(
            ShutdownSignal::Sigterm,
            &mut controller,
            &mut finalizer,
            SHUTDOWN_NS,
        )
        .expect("handle sigterm");

    let SupervisorShutdownSignalAction::Graceful(graceful) = dispatch.action else {
        panic!("expected graceful shutdown");
    };
    assert_eq!(graceful.runtime.kind, ShutdownKind::Poweroff);
    assert!(finalizer.calls.is_empty());
}

#[test]
fn sigpwr_begins_graceful_poweroff_shutdown() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut finalizer = SignalFinalizer::default();

    let dispatch = supervisor
        .handle_shutdown_signal(
            ShutdownSignal::Sigpwr,
            &mut controller,
            &mut finalizer,
            SHUTDOWN_NS,
        )
        .expect("handle sigpwr");

    let SupervisorShutdownSignalAction::Graceful(graceful) = dispatch.action else {
        panic!("expected graceful shutdown");
    };
    assert_eq!(graceful.runtime.kind, ShutdownKind::Poweroff);
    assert!(finalizer.calls.is_empty());
}

#[test]
fn third_sigint_in_window_forces_immediate_reboot() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut finalizer = SignalFinalizer::default();

    let first = supervisor
        .handle_shutdown_signal(
            ShutdownSignal::Sigint,
            &mut controller,
            &mut finalizer,
            SHUTDOWN_NS,
        )
        .expect("first sigint");
    assert!(matches!(
        first.action,
        SupervisorShutdownSignalAction::Graceful(_),
    ));

    let second = supervisor
        .handle_shutdown_signal(
            ShutdownSignal::Sigint,
            &mut controller,
            &mut finalizer,
            SHUTDOWN_NS + 1_000_000_000,
        )
        .expect("second sigint");
    assert_eq!(
        second.action,
        SupervisorShutdownSignalAction::AlreadyInProgress {
            kind: ShutdownKind::Reboot,
        },
    );

    let third = supervisor
        .handle_shutdown_signal(
            ShutdownSignal::Sigint,
            &mut controller,
            &mut finalizer,
            SHUTDOWN_NS + 2_000_000_000,
        )
        .expect("third sigint");

    let SupervisorShutdownSignalAction::Forced(forced) = third.action else {
        panic!("expected forced reboot");
    };
    assert_eq!(
        forced
            .killed_services
            .iter()
            .map(|kill| kill.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app", "db", "draining"],
    );
    assert_eq!(finalizer.calls, vec![SignalCall::Sync, SignalCall::Reboot],);
    assert_eq!(
        supervisor.shutdown().expect("shutdown").kind,
        ShutdownKind::Reboot,
    );
}

#[test]
fn pid1_signal_fd_shutdown_read_enters_supervisor_shutdown_path() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut finalizer = SignalFinalizer::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);

    let turn = supervisor
        .handle_pid1_signal_fd_read(
            LinuxSignalFdRead::Shutdown(ShutdownSignal::Sigterm),
            &mut clock,
            &mut controller,
            &mut finalizer,
        )
        .expect("signal fd turn");

    let SupervisorPid1SignalFdTurn::Shutdown(dispatch) = turn else {
        panic!("expected shutdown signal turn");
    };
    let SupervisorShutdownSignalAction::Graceful(graceful) = dispatch.action else {
        panic!("expected graceful shutdown");
    };
    assert_eq!(graceful.runtime.kind, ShutdownKind::Poweroff);
    assert!(supervisor.shutdown().is_some());
}

#[test]
fn pid1_signal_fd_other_signal_does_not_consume_clock_or_mutate_shutdown() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut finalizer = SignalFinalizer::default();
    let mut clock = ScriptedClock::new([]);

    let turn = supervisor
        .handle_pid1_signal_fd_read(
            LinuxSignalFdRead::Other {
                signal: libc::SIGCHLD,
            },
            &mut clock,
            &mut controller,
            &mut finalizer,
        )
        .expect("signal fd turn");

    assert_eq!(
        turn,
        SupervisorPid1SignalFdTurn::Other {
            signal: libc::SIGCHLD,
        },
    );
    assert!(supervisor.shutdown().is_none());
}

#[test]
fn pid1_signal_fd_would_block_does_not_consume_clock_or_mutate_shutdown() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut finalizer = SignalFinalizer::default();
    let mut clock = ScriptedClock::new([]);

    let turn = supervisor
        .handle_pid1_signal_fd_read(
            LinuxSignalFdRead::WouldBlock,
            &mut clock,
            &mut controller,
            &mut finalizer,
        )
        .expect("signal fd turn");

    assert_eq!(turn, SupervisorPid1SignalFdTurn::WouldBlock);
    assert!(supervisor.shutdown().is_none());
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SignalCall {
    Sync,
    Reboot,
}

#[derive(Debug, Default)]
struct SignalFinalizer {
    calls: Vec<SignalCall>,
}

impl ShutdownFinalizer for SignalFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        panic!("signal forced reboot must not snapshot mounts");
    }

    fn unmount(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        panic!("signal forced reboot must not unmount");
    }

    fn remount_readonly(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        panic!("signal forced reboot must not remount");
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(SignalCall::Sync);
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        assert_eq!(kind, ShutdownKind::Reboot);
        self.calls.push(SignalCall::Reboot);
        Ok(())
    }
}
