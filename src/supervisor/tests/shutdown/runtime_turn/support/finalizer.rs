use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::shutdown::ShutdownKind;

#[derive(Debug, Default)]
pub(crate) struct RuntimeFinalizer {
    pub(crate) calls: Vec<RuntimeFinalizerCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeFinalizerCall {
    Snapshot,
    Unmount(String),
    Remount(String),
    Sync,
    Reboot(ShutdownKind),
}

impl ShutdownFinalizer for RuntimeFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        self.calls.push(RuntimeFinalizerCall::Snapshot);
        Ok(vec!["/".to_string()])
    }

    fn unmount(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        self.calls
            .push(RuntimeFinalizerCall::Unmount(mount_point.to_string()));
        Ok(())
    }

    fn remount_readonly(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        self.calls
            .push(RuntimeFinalizerCall::Remount(mount_point.to_string()));
        Ok(())
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(RuntimeFinalizerCall::Sync);
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        self.calls.push(RuntimeFinalizerCall::Reboot(kind));
        Ok(())
    }
}
