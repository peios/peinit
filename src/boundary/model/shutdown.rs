use crate::shutdown::ShutdownKind;

use super::error::BoundaryError;

pub trait ShutdownFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError>;

    fn unmount(&mut self, mount_point: &str) -> Result<(), BoundaryError>;

    fn remount_readonly(&mut self, mount_point: &str) -> Result<(), BoundaryError>;

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError>;

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError>;
}

pub trait ShutdownDeadlineTimer {
    fn arm_absolute_ns(&mut self, deadline_ns: u64) -> Result<(), BoundaryError>;

    fn disarm(&mut self) -> Result<(), BoundaryError>;
}
