use std::collections::VecDeque;

use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::shutdown::{ShutdownDeadlineKind, ShutdownFinalizationState, ShutdownKind};

use super::super::TestProcessController;
use super::SHUTDOWN_NS;
use super::fixture::{DRAINING_STOP_DEADLINE_NS, drive_shutdown_to_ready, shutdown_fixture};

const POST_KILL_TIMEOUT_NS: u64 = 5_000_000_000;

#[test]
fn next_shutdown_deadline_reports_the_earliest_scheduled_shutdown_work() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();

    assert_eq!(supervisor.next_shutdown_deadline(), None);

    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    let deadline = supervisor
        .next_shutdown_deadline()
        .expect("shutdown deadline");
    assert_eq!(deadline.due_at_ns, DRAINING_STOP_DEADLINE_NS);
    assert_eq!(
        deadline.kind,
        ShutdownDeadlineKind::StopTimeout {
            service: "draining".to_string(),
        },
    );
}

#[test]
fn drive_shutdown_before_next_deadline_is_a_noop() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut finalizer = TestDriveFinalizer::new(["/"]);
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    controller.cgroup_kills.clear();

    let dispatch = supervisor
        .drive_shutdown(
            &mut controller,
            Some(&mut finalizer),
            DRAINING_STOP_DEADLINE_NS - 1,
        )
        .expect("drive shutdown");

    assert_eq!(dispatch, None);
    assert!(controller.cgroup_kills.is_empty());
    assert!(finalizer.calls.is_empty());
}

#[test]
fn drive_shutdown_processes_due_stop_timeout_and_schedules_post_kill_deadline() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut finalizer = TestDriveFinalizer::new(["/"]);
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    controller.cgroup_kills.clear();

    let dispatch = supervisor
        .drive_shutdown(
            &mut controller,
            Some(&mut finalizer),
            DRAINING_STOP_DEADLINE_NS,
        )
        .expect("drive shutdown")
        .expect("timeout dispatch");
    let timeout = dispatch.timeout.expect("timeout work");

    assert!(dispatch.finalization.is_none());
    assert!(!timeout.global_timeout);
    assert_eq!(timeout.cgroup_kills.len(), 1);
    assert_eq!(timeout.cgroup_kills[0].service, "draining");
    assert_eq!(
        timeout.cgroup_kills[0].cgroup_id,
        "/sys/fs/cgroup/peinit/draining",
    );
    assert!(timeout.abandoned.is_empty());
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/draining".to_string()],
    );
    assert!(finalizer.calls.is_empty());

    let next = supervisor.next_shutdown_deadline().expect("next deadline");
    assert_eq!(
        next.due_at_ns,
        DRAINING_STOP_DEADLINE_NS + POST_KILL_TIMEOUT_NS,
    );
    assert_eq!(
        next.kind,
        ShutdownDeadlineKind::PostKillTimeout {
            service: "draining".to_string(),
        },
    );
}

#[test]
fn drive_shutdown_finalizes_when_services_are_ready() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    drive_shutdown_to_ready(&mut supervisor, ShutdownKind::Reboot, &mut controller);
    let mut finalizer = TestDriveFinalizer::new(["/", "/run"]);

    let dispatch = supervisor
        .drive_shutdown(&mut controller, Some(&mut finalizer), SHUTDOWN_NS + 100)
        .expect("drive shutdown")
        .expect("finalization dispatch");

    assert!(dispatch.timeout.is_none());
    assert_eq!(
        dispatch.finalization.expect("finalization").finalization,
        ShutdownFinalizationState::Completed,
    );
    assert_eq!(
        finalizer.calls,
        vec![
            DriveCall::SnapshotMounts,
            DriveCall::Unmount("/run".to_string()),
            DriveCall::RemountReadonly("/".to_string()),
            DriveCall::Sync,
            DriveCall::Reboot(ShutdownKind::Reboot),
        ],
    );
}

#[test]
fn drive_shutdown_retries_failed_final_action_only_when_due() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    drive_shutdown_to_ready(&mut supervisor, ShutdownKind::Poweroff, &mut controller);
    let mut finalizer =
        TestDriveFinalizer::new(["/", "/run"]).with_reboot_results([Err("first failed"), Ok(())]);

    let failed = supervisor
        .drive_shutdown(&mut controller, Some(&mut finalizer), SHUTDOWN_NS + 100)
        .expect("drive shutdown")
        .expect("failed finalization")
        .finalization
        .expect("finalization");
    let next_retry_at_ns = SHUTDOWN_NS + 1_000_000_100;
    assert_eq!(
        failed.finalization,
        ShutdownFinalizationState::Failed {
            message: "Shutdown(\"first failed\")".to_string(),
            next_retry_at_ns,
        },
    );
    assert_eq!(
        supervisor
            .next_shutdown_deadline()
            .expect("retry deadline")
            .kind,
        ShutdownDeadlineKind::FinalActionRetry,
    );

    let not_due = supervisor
        .drive_shutdown(&mut controller, Some(&mut finalizer), next_retry_at_ns - 1)
        .expect("drive before retry");
    assert_eq!(not_due, None);
    let calls_before_retry = finalizer.calls.len();

    let retried = supervisor
        .drive_shutdown(&mut controller, Some(&mut finalizer), next_retry_at_ns)
        .expect("drive retry")
        .expect("retry dispatch")
        .finalization
        .expect("finalization");

    assert_eq!(retried.finalization, ShutdownFinalizationState::Completed);
    assert_eq!(
        &finalizer.calls[calls_before_retry..],
        &[DriveCall::Sync, DriveCall::Reboot(ShutdownKind::Poweroff),],
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DriveCall {
    SnapshotMounts,
    Unmount(String),
    RemountReadonly(String),
    Sync,
    Reboot(ShutdownKind),
}

#[derive(Debug)]
struct TestDriveFinalizer {
    mounts: Vec<String>,
    reboot_results: VecDeque<Result<(), &'static str>>,
    calls: Vec<DriveCall>,
}

impl TestDriveFinalizer {
    fn new<const N: usize>(mounts: [&str; N]) -> Self {
        Self {
            mounts: mounts.into_iter().map(ToString::to_string).collect(),
            reboot_results: VecDeque::from([Ok(())]),
            calls: Vec::new(),
        }
    }

    fn with_reboot_results<const N: usize>(
        mut self,
        results: [Result<(), &'static str>; N],
    ) -> Self {
        self.reboot_results = results.into();
        self
    }
}

impl ShutdownFinalizer for TestDriveFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        self.calls.push(DriveCall::SnapshotMounts);
        Ok(self.mounts.clone())
    }

    fn unmount(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        self.calls.push(DriveCall::Unmount(mount_point.to_string()));
        Ok(())
    }

    fn remount_readonly(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        self.calls
            .push(DriveCall::RemountReadonly(mount_point.to_string()));
        Ok(())
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(DriveCall::Sync);
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        self.calls.push(DriveCall::Reboot(kind));
        match self.reboot_results.pop_front().unwrap_or(Ok(())) {
            Ok(()) => Ok(()),
            Err(message) => Err(BoundaryError::Shutdown(message.to_string())),
        }
    }
}
