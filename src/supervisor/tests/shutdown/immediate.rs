use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::shutdown::{CleanupActionResult, ShutdownFinalizationState, ShutdownKind};

use super::super::TestProcessController;
use super::SHUTDOWN_NS;
use super::fixture::shutdown_fixture;

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImmediateCall {
    Sync,
    Reboot,
}

#[derive(Debug, Default)]
struct ImmediateFinalizer {
    calls: Vec<ImmediateCall>,
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
        Ok(())
    }
}
