use std::collections::VecDeque;

use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::job::JobExit;
use crate::shutdown::{
    CleanupActionResult, ShutdownDeadlineKind, ShutdownFinalizationState, ShutdownKind,
};

use super::super::TestProcessController;
use super::SHUTDOWN_NS;
use super::fixture::{job_for, shutdown_fixture};

#[test]
fn forced_shutdown_kills_process_cgroups_and_syncs_reboots_without_mount_cleanup() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut finalizer = ImmediateFinalizer::default();

    let dispatch = supervisor
        .force_reboot_shutdown(&mut controller, &mut finalizer, SHUTDOWN_NS)
        .expect("forced shutdown");

    assert_eq!(
        dispatch
            .killed_services
            .iter()
            .map(|kill| kill.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app", "booting", "db", "draining"],
    );
    assert_eq!(
        controller.cgroup_kills,
        vec![
            "/sys/fs/cgroup/peinit/app",
            "/sys/fs/cgroup/peinit/booting",
            "/sys/fs/cgroup/peinit/db",
            "/sys/fs/cgroup/peinit/draining",
        ],
    );
    assert_eq!(
        dispatch.finalization.finalization,
        ShutdownFinalizationState::Completed,
    );
    assert_eq!(
        dispatch.finalization.report.snapshot_mounts,
        CleanupActionResult::Ok,
    );
    assert!(dispatch.finalization.report.mount_results.is_empty());
    assert_eq!(
        finalizer.calls,
        vec![ImmediateCall::Sync, ImmediateCall::Reboot]
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").kind,
        ShutdownKind::Reboot,
    );
}

#[test]
fn critical_reboot_syncs_and_reboots_without_service_kill_or_mount_cleanup() {
    let mut supervisor = shutdown_fixture();
    let mut finalizer = ImmediateFinalizer::default();

    let dispatch = supervisor
        .critical_reboot(&mut finalizer, SHUTDOWN_NS)
        .expect("critical reboot");

    assert_eq!(dispatch.finalization, ShutdownFinalizationState::Completed);
    assert!(dispatch.report.mount_results.is_empty());
    assert_eq!(
        finalizer.calls,
        vec![ImmediateCall::Sync, ImmediateCall::Reboot]
    );
    assert_eq!(
        supervisor.shutdown().expect("shutdown").kind,
        ShutdownKind::Reboot,
    );
}

// PEI-1087. The forced reboot installs an empty plan already in the Failed
// finalisation state and attempts sync and reboot at once. When reboot(2)
// returned, the reaps of the SIGKILLed services arrived afterwards and
// advance_shutdown_progress — no waves left, nothing pending — set the
// finalisation to Ready over the top of Failed. The next drive then ran the
// full graceful finalisation (seed write, unmounts, remounts) instead of
// retrying the action that failed.
#[test]
fn a_failed_forced_reboot_stays_failed_through_the_reaps_and_retries_only_sync_and_reboot() {
    let mut supervisor = shutdown_fixture();
    let app_job = job_for(&supervisor, "app");
    let db_job = job_for(&supervisor, "db");
    let draining_job = job_for(&supervisor, "draining");
    let mut controller = TestProcessController::default();
    let mut finalizer =
        ImmediateFinalizer::default().with_reboot_results([Err("reboot returned"), Ok(())]);

    let forced = supervisor
        .force_reboot_shutdown(&mut controller, &mut finalizer, SHUTDOWN_NS)
        .expect("forced shutdown");
    let failed = ShutdownFinalizationState::Failed {
        message: "Shutdown(\"reboot returned\")".to_string(),
        next_retry_at_ns: SHUTDOWN_NS + 1_000_000_000,
    };
    assert_eq!(forced.finalization.finalization, failed);

    // The killed services are reaped while the retry is pending.
    for (job, at) in [(app_job, 1), (db_job, 2), (draining_job, 3)] {
        let reap = supervisor
            .fail_running_shutdown_job(
                job,
                SHUTDOWN_NS + at,
                Some(JobExit::Signal(libc::SIGKILL)),
                "killed by forced reboot",
                &mut controller,
            )
            .expect("reap a killed service");
        assert!(reap.next_wave.is_empty());
        assert_eq!(
            reap.finalization, failed,
            "a reap must not turn a failed final action back into a fresh finalisation",
        );
    }
    assert_eq!(
        supervisor
            .next_shutdown_deadline()
            .expect("retry deadline")
            .kind,
        ShutdownDeadlineKind::FinalActionRetry,
    );

    // The retry, when due, is the same action: sync and reboot, and nothing
    // the ImmediateFinalizer refuses to do.
    let calls_before_retry = finalizer.calls.len();
    let retried = supervisor
        .drive_shutdown(&mut controller, &mut finalizer, SHUTDOWN_NS + 1_000_000_000)
        .expect("drive retry")
        .expect("retry dispatch")
        .finalization
        .expect("finalization");
    assert_eq!(retried.finalization, ShutdownFinalizationState::Completed);
    assert_eq!(
        &finalizer.calls[calls_before_retry..],
        &[ImmediateCall::Sync, ImmediateCall::Reboot],
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImmediateCall {
    Sync,
    Reboot,
}

#[derive(Debug, Default)]
struct ImmediateFinalizer {
    calls: Vec<ImmediateCall>,
    reboot_results: VecDeque<Result<(), &'static str>>,
}

impl ImmediateFinalizer {
    fn with_reboot_results<const N: usize>(
        mut self,
        results: [Result<(), &'static str>; N],
    ) -> Self {
        self.reboot_results = results.into();
        self
    }
}

impl ShutdownFinalizer for ImmediateFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        panic!("immediate shutdown must not snapshot mounts");
    }

    fn unmount(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        panic!("immediate shutdown must not unmount");
    }

    fn remount_readonly(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        panic!("immediate shutdown must not remount filesystems");
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(ImmediateCall::Sync);
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        assert_eq!(kind, ShutdownKind::Reboot);
        self.calls.push(ImmediateCall::Reboot);
        match self.reboot_results.pop_front().unwrap_or(Ok(())) {
            Ok(()) => Ok(()),
            Err(message) => Err(BoundaryError::Shutdown(message.to_string())),
        }
    }
}
