//! A job that leaves the store releases its pidfd for the runtime to close.
//!
//! The job record owns the pidfd from `start`, and before PEI-816 the record
//! was simply dropped on an ordinary reap: PID 1 held one more
//! `anon_inode:[pidfd]` per activation for the rest of the boot, on the one
//! process whose descriptor table can never be cleared by a restart.

use crate::boundary::{BoundaryError, ChildExitStatus, ChildReap, ShutdownFinalizer};
use crate::shutdown::ShutdownKind;
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};

#[derive(Debug, Default)]
struct QuietFinalizer;

impl ShutdownFinalizer for QuietFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        Ok(Vec::new())
    }
    fn unmount(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        Ok(())
    }
    fn remount_readonly(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        Ok(())
    }
    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        Ok(())
    }
    fn reboot(&mut self, _kind: ShutdownKind) -> Result<(), BoundaryError> {
        Ok(())
    }
}

#[test]
fn reaping_a_service_main_job_releases_its_pidfd_once() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![alive_service("app")]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let job_id = supervisor.pending_launch_jobs()[0];

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 37)]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);
    supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("launch dispatch");
    assert_eq!(
        supervisor.jobs().get(job_id).expect("job").pidfd,
        Some(37),
        "the record owns the launched process's pidfd"
    );
    assert!(
        supervisor.take_released_pidfds().is_empty(),
        "a running job's pidfd is still in use"
    );

    let mut controller = TestProcessController::default();
    let mut finalizer = QuietFinalizer;
    supervisor
        .apply_reaped_child(
            ChildReap {
                pid: 4242,
                status: ChildExitStatus::Exited { code: 0 },
            },
            APP_LAUNCH_NS + 1,
            &mut controller,
            Some(&mut finalizer),
        )
        .expect("reap");

    assert!(supervisor.jobs().get(job_id).is_none());
    assert_eq!(supervisor.take_released_pidfds(), vec![37]);
    assert!(
        supervisor.take_released_pidfds().is_empty(),
        "a released pidfd is handed to the runtime exactly once"
    );
}
