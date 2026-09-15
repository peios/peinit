use crate::shutdown::ShutdownKind;

use super::error::BoundaryError;

pub trait ShutdownFinalizer {
    fn save_random_seed(&mut self) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError>;

    fn unmount(&mut self, mount_point: &str) -> Result<(), BoundaryError>;

    fn remount_readonly(&mut self, mount_point: &str) -> Result<(), BoundaryError>;

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError>;

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError>;

    /// `CLOCK_MONOTONIC` now, read after a final action has returned.
    ///
    /// A failed final action is retried no more than once a second (TRM
    /// §12.4), and the second is measured from the attempt, not from the
    /// start of the turn that made it: the seed write and unmounts before
    /// `reboot(2)` take tens of milliseconds, and a retry timed from before
    /// them came early by that much (PEI-1088). `None` when the finalizer has
    /// no clock, in which case the retry is timed from the moment the caller
    /// was given.
    fn monotonic_now_ns(&mut self) -> Option<u64> {
        None
    }
}

pub trait ShutdownDeadlineTimer {
    fn arm_absolute_ns(&mut self, deadline_ns: u64) -> Result<(), BoundaryError>;

    fn disarm(&mut self) -> Result<(), BoundaryError>;
}
