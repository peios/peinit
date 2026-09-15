use std::collections::{BTreeSet, VecDeque};

use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::shutdown::{
    CleanupActionResult, MountCleanupResult, ShutdownFinalizationState, ShutdownKind,
};
use crate::supervisor::SupervisorError;

use super::super::TestProcessController;
use super::SHUTDOWN_NS;
use super::fixture::{drive_shutdown_to_ready, shutdown_fixture};

#[test]
fn finalize_shutdown_unmounts_deepest_first_remounts_failures_syncs_and_reboots() {
    let mut supervisor = ready_supervisor(ShutdownKind::Reboot);
    let mut finalizer = TestShutdownFinalizer::new([
        "/",
        "/sys",
        "/sys/fs/cgroup",
        "/run",
        "/proc",
        "/dev/pts",
        "/dev",
    ])
    .fail_unmount("/run");

    let dispatch = supervisor.finalize_shutdown(&mut finalizer, SHUTDOWN_NS + 100);

    let dispatch = dispatch.expect("finalize shutdown");
    assert_eq!(dispatch.finalization, ShutdownFinalizationState::Completed);
    assert_eq!(dispatch.report.random_seed, CleanupActionResult::Ok);
    assert_eq!(dispatch.report.snapshot_mounts, CleanupActionResult::Ok);
    assert_eq!(
        dispatch
            .report
            .mount_results
            .iter()
            .map(|result| result.mount_point.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/sys/fs/cgroup",
            "/dev/pts",
            "/dev",
            "/proc",
            "/run",
            "/sys"
        ],
    );
    assert_eq!(
        dispatch.report.mount_results[4],
        MountCleanupResult {
            mount_point: "/run".to_string(),
            unmount: CleanupActionResult::Failed("Shutdown(\"unmount /run failed\")".to_string(),),
            remount_readonly: Some(CleanupActionResult::Ok),
        },
    );
    assert_eq!(dispatch.report.root_remount, CleanupActionResult::Ok);
    assert_eq!(dispatch.report.sync_result, CleanupActionResult::Ok);
    assert_eq!(dispatch.report.reboot_result, CleanupActionResult::Ok);
    assert_eq!(
        finalizer.calls,
        vec![
            FinalizerCall::SaveRandomSeed,
            FinalizerCall::SnapshotMounts,
            FinalizerCall::Unmount("/sys/fs/cgroup".to_string()),
            FinalizerCall::Unmount("/dev/pts".to_string()),
            FinalizerCall::Unmount("/dev".to_string()),
            FinalizerCall::Unmount("/proc".to_string()),
            FinalizerCall::Unmount("/run".to_string()),
            FinalizerCall::RemountReadonly("/run".to_string()),
            FinalizerCall::Unmount("/sys".to_string()),
            FinalizerCall::RemountReadonly("/".to_string()),
            FinalizerCall::Sync,
            FinalizerCall::Reboot(ShutdownKind::Reboot),
        ],
    );
}

// PEI-1088. TRM §12.4: a failed final action is retried no more than once a
// second. The second runs from the attempt — the finalizer's clock after
// reboot(2) returned — not from the start of the finalising turn, which is
// the seed write and the unmounts earlier.
#[test]
fn failed_final_action_enters_retry_state_and_retries_only_sync_and_reboot_when_due() {
    let mut supervisor = ready_supervisor(ShutdownKind::Poweroff);
    // The turn began at +100; the attempt returned 20 ms later.
    let attempt_returned_at_ns = SHUTDOWN_NS + 20_000_100;
    let mut finalizer = TestShutdownFinalizer::new(["/", "/run"])
        .with_reboot_results([Err("first reboot failed"), Ok(())])
        .with_clock_after_attempt(attempt_returned_at_ns);

    let failed = supervisor
        .finalize_shutdown(&mut finalizer, SHUTDOWN_NS + 100)
        .expect("first finalization attempt");

    let next_retry_at_ns = attempt_returned_at_ns + 1_000_000_000;
    assert_eq!(
        failed.finalization,
        ShutdownFinalizationState::Failed {
            message: "Shutdown(\"first reboot failed\")".to_string(),
            next_retry_at_ns,
        },
    );
    assert_eq!(
        failed.report.reboot_result,
        CleanupActionResult::Failed("Shutdown(\"first reboot failed\")".to_string()),
    );

    // A second after the turn began is not yet a second after the attempt.
    for not_due_ns in [SHUTDOWN_NS + 1_000_000_100, next_retry_at_ns - 1] {
        let retry_not_due = supervisor
            .finalize_shutdown(&mut finalizer, not_due_ns)
            .expect_err("retry not due");
        assert!(matches!(
            retry_not_due,
            SupervisorError::Shutdown(
                crate::shutdown::ShutdownError::FinalActionRetryNotDue { .. }
            )
        ));
    }
    let calls_before_retry = finalizer.calls.len();

    let retry = supervisor
        .finalize_shutdown(&mut finalizer, next_retry_at_ns)
        .expect("retry final action");

    assert_eq!(retry.finalization, ShutdownFinalizationState::Completed);
    assert!(retry.report.mount_results.is_empty());
    assert_eq!(
        &finalizer.calls[calls_before_retry..],
        &[
            FinalizerCall::Sync,
            FinalizerCall::Reboot(ShutdownKind::Poweroff),
        ],
    );
}

#[test]
fn random_seed_save_failure_is_reported_but_does_not_block_final_action() {
    let mut supervisor = ready_supervisor(ShutdownKind::Poweroff);
    let mut finalizer = TestShutdownFinalizer::new(["/", "/run"]).fail_random_seed_save();

    let dispatch = supervisor
        .finalize_shutdown(&mut finalizer, SHUTDOWN_NS + 100)
        .expect("finalize shutdown");

    assert_eq!(dispatch.finalization, ShutdownFinalizationState::Completed);
    assert_eq!(
        dispatch.report.random_seed,
        CleanupActionResult::Failed("Shutdown(\"random seed save failed\")".to_string()),
    );
    assert_eq!(
        &finalizer.calls,
        &[
            FinalizerCall::SaveRandomSeed,
            FinalizerCall::SnapshotMounts,
            FinalizerCall::Unmount("/run".to_string()),
            FinalizerCall::RemountReadonly("/".to_string()),
            FinalizerCall::Sync,
            FinalizerCall::Reboot(ShutdownKind::Poweroff),
        ],
    );
}

fn ready_supervisor(kind: ShutdownKind) -> crate::supervisor::Supervisor {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    drive_shutdown_to_ready(&mut supervisor, kind, &mut controller);
    supervisor
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FinalizerCall {
    SaveRandomSeed,
    SnapshotMounts,
    Unmount(String),
    RemountReadonly(String),
    Sync,
    Reboot(ShutdownKind),
}

#[derive(Debug)]
struct TestShutdownFinalizer {
    mounts: Vec<String>,
    failed_unmounts: BTreeSet<String>,
    fail_random_seed_save: bool,
    reboot_results: VecDeque<Result<(), &'static str>>,
    /// What the finalizer's clock reads after an attempt; `None` is a
    /// finalizer without a clock.
    clock_after_attempt_ns: Option<u64>,
    calls: Vec<FinalizerCall>,
}

impl TestShutdownFinalizer {
    fn new<const N: usize>(mounts: [&str; N]) -> Self {
        Self {
            mounts: mounts.into_iter().map(ToString::to_string).collect(),
            failed_unmounts: BTreeSet::new(),
            fail_random_seed_save: false,
            reboot_results: VecDeque::from([Ok(())]),
            clock_after_attempt_ns: None,
            calls: Vec::new(),
        }
    }

    fn with_clock_after_attempt(mut self, now_ns: u64) -> Self {
        self.clock_after_attempt_ns = Some(now_ns);
        self
    }

    fn fail_unmount(mut self, mount_point: &str) -> Self {
        self.failed_unmounts.insert(mount_point.to_string());
        self
    }

    fn fail_random_seed_save(mut self) -> Self {
        self.fail_random_seed_save = true;
        self
    }

    fn with_reboot_results<const N: usize>(
        mut self,
        results: [Result<(), &'static str>; N],
    ) -> Self {
        self.reboot_results = results.into();
        self
    }
}

impl ShutdownFinalizer for TestShutdownFinalizer {
    fn save_random_seed(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(FinalizerCall::SaveRandomSeed);
        if self.fail_random_seed_save {
            Err(BoundaryError::Shutdown(
                "random seed save failed".to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        self.calls.push(FinalizerCall::SnapshotMounts);
        Ok(self.mounts.clone())
    }

    fn unmount(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        self.calls
            .push(FinalizerCall::Unmount(mount_point.to_string()));
        if self.failed_unmounts.contains(mount_point) {
            Err(BoundaryError::Shutdown(format!(
                "unmount {mount_point} failed",
            )))
        } else {
            Ok(())
        }
    }

    fn remount_readonly(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        self.calls
            .push(FinalizerCall::RemountReadonly(mount_point.to_string()));
        Ok(())
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(FinalizerCall::Sync);
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        self.calls.push(FinalizerCall::Reboot(kind));
        match self.reboot_results.pop_front().unwrap_or(Ok(())) {
            Ok(()) => Ok(()),
            Err(message) => Err(BoundaryError::Shutdown(message.to_string())),
        }
    }

    fn monotonic_now_ns(&mut self) -> Option<u64> {
        assert!(
            matches!(self.calls.last(), Some(FinalizerCall::Reboot(_))),
            "the clock is read after the attempt, not before it",
        );
        self.clock_after_attempt_ns
    }
}
