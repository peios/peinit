//! A child that exits before its setup status is read must not lose its exit.
//!
//! peinit resolves a reaped child to a job by pid, and a job only carries a pid
//! once its setup status has been processed. A short-lived process — a one-line
//! `ExecStartPre` hook, say — can be gone before peinit gets back to the setup
//! pipe. Dropping that exit is unrecoverable: the job is started moments later
//! against a pid that is already reaped, and no second SIGCHLD is ever coming,
//! so the job stays Running for ever and its service never leaves Starting.
//!
//! That is PEI-601/PEI-605: trustd's hook lost this race on most boots, parked
//! in Starting, timed out at 30s, and the restart the timeout produced was the
//! job whose launch took PID 1 out of its runtime loop.

use crate::boundary::{
    BoundaryError, ChildExitStatus, ChildReap, ProcessSetupStatus, ShutdownFinalizer,
};
use crate::job::JobState;
use crate::service::runtime::ServiceState;
use crate::shutdown::ShutdownKind;
use crate::supervisor::{
    Supervisor, SupervisorChildReapTurn, SupervisorServiceLaunchDispatch, SupervisorSettings,
};

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

/// Launch the boot service into a pending setup, leaving the job in `Created`
/// with `pid` live and unrecorded — the window this whole module is about.
fn launch_into_pending_setup(supervisor: &mut Supervisor, pid: u32, setup_fd: i32) {
    let mut pending = process(pid, 9);
    pending.setup_status_fd = Some(setup_fd);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![pending]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);
    let launch = supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("pending launch dispatch");
    assert!(matches!(
        launch,
        SupervisorServiceLaunchDispatch::PendingSetup(_)
    ));
}

fn booted() -> (Supervisor, crate::ids::JobId) {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![alive_service("app")]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let job_id = supervisor.pending_launch_jobs()[0];
    (supervisor, job_id)
}

#[test]
fn an_exit_reaped_before_the_setup_status_is_held_and_replayed() {
    let (mut supervisor, job_id) = booted();
    launch_into_pending_setup(&mut supervisor, 4242, 55);
    assert_eq!(
        supervisor.jobs().get(job_id).expect("job").state,
        JobState::Created,
        "the job must not yet carry a pid — that is the premise",
    );

    // The process exits before peinit reads its setup pipe.
    let mut controller = TestProcessController::default();
    let mut finalizer = QuietFinalizer;
    let reap = supervisor
        .apply_reaped_child(
            ChildReap {
                pid: 4242,
                status: ChildExitStatus::Exited { code: 0 },
            },
            APP_LAUNCH_NS + 1,
            &mut controller,
            &mut finalizer,
        )
        .expect("reap");
    assert!(
        matches!(reap, SupervisorChildReapTurn::DeferredUntilSetup { .. }),
        "an exit whose pid belongs to a pending setup must be held, not dropped",
    );
    assert!(
        supervisor.take_ready_deferred_reaps().is_empty(),
        "nothing to replay while the job still has no pid",
    );

    // The setup status arrives afterwards and gives the job its pid.
    supervisor
        .process_pending_process_setup_status(
            55,
            ProcessSetupStatus::ExecSucceeded,
            APP_LAUNCH_NS + 2,
            &mut controller,
        )
        .expect("complete setup");
    assert_eq!(
        supervisor.jobs().get(job_id).expect("job").state,
        JobState::Running
    );

    // Now the held exit is replayable, and replaying it retires the job.
    let ready = supervisor.take_ready_deferred_reaps();
    assert_eq!(
        ready,
        vec![ChildReap {
            pid: 4242,
            status: ChildExitStatus::Exited { code: 0 },
        }]
    );
    let replayed = supervisor
        .apply_reaped_child(ready[0], APP_LAUNCH_NS + 3, &mut controller, &mut finalizer)
        .expect("replay");
    assert!(matches!(replayed, SupervisorChildReapTurn::Tracked { .. }));
    assert!(
        supervisor.jobs().get(job_id).is_none(),
        "the job is retired rather than left Running against a reaped pid",
    );
    assert_ne!(
        supervisor.service_status("app").expect("app status").state,
        ServiceState::Starting,
        "the service must not be parked in Starting",
    );
}

/// The regression itself: without the hold, the service parks for ever.
#[test]
fn the_service_does_not_park_in_starting_when_it_loses_the_race() {
    let (mut supervisor, job_id) = booted();
    launch_into_pending_setup(&mut supervisor, 4242, 55);

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
            &mut finalizer,
        )
        .expect("reap");
    supervisor
        .process_pending_process_setup_status(
            55,
            ProcessSetupStatus::ExecSucceeded,
            APP_LAUNCH_NS + 2,
            &mut controller,
        )
        .expect("complete setup");
    for child in supervisor.take_ready_deferred_reaps() {
        supervisor
            .apply_reaped_child(child, APP_LAUNCH_NS + 3, &mut controller, &mut finalizer)
            .expect("replay");
    }

    assert!(
        supervisor.jobs().get(job_id).is_none(),
        "a job left Running here is the hang: no second SIGCHLD is coming",
    );
}

/// A pid peinit never launched stays untracked. PID 1 reaps orphans it did not
/// start, and the timer last-run writes are claimed off exactly this path
/// (`claim_timer_last_run_write_exits`) — holding those would break them.
#[test]
fn a_pid_with_no_pending_setup_is_still_untracked() {
    let (mut supervisor, _job_id) = booted();
    launch_into_pending_setup(&mut supervisor, 4242, 55);

    let mut controller = TestProcessController::default();
    let mut finalizer = QuietFinalizer;
    let reap = supervisor
        .apply_reaped_child(
            ChildReap {
                pid: 9999,
                status: ChildExitStatus::Exited { code: 0 },
            },
            APP_LAUNCH_NS + 1,
            &mut controller,
            &mut finalizer,
        )
        .expect("reap");
    assert!(matches!(reap, SupervisorChildReapTurn::Untracked { .. }));
    assert!(supervisor.take_ready_deferred_reaps().is_empty());
}
