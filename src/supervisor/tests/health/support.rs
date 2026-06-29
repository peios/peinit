use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::service::ErrorControl;
use crate::shutdown::ShutdownKind;
use crate::supervisor::{
    Supervisor, SupervisorHealthCheckLaunchDispatch, SupervisorHealthCheckLaunchResult,
    SupervisorSettings,
};

use super::super::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};

pub(super) const HEALTH_INTERVAL_SECS: u64 = 1;
pub(super) const HEALTH_TIMEOUT_SECS: u64 = 5;

pub(super) fn active_health_supervisor(retries: u32) -> (Supervisor, u64) {
    let mut app = alive_service("app");
    app.health_check = Some("/usr/bin/app-health --quick".to_string());
    app.health_check_interval_secs = HEALTH_INTERVAL_SECS;
    app.health_check_timeout_secs = HEALTH_TIMEOUT_SECS;
    app.health_check_retries = retries;

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8000, 50)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch");

    let first_due = APP_LAUNCH_NS + HEALTH_INTERVAL_SECS * 1_000_000_000;
    assert_eq!(
        supervisor
            .next_health_check_interval()
            .expect("health interval")
            .due_at_ns,
        first_due,
    );
    (supervisor, first_due)
}

pub(super) fn critical_active_health_supervisor() -> (Supervisor, u64) {
    let mut app = alive_service("app");
    app.health_check = Some("/usr/bin/app-health --quick".to_string());
    app.health_check_interval_secs = HEALTH_INTERVAL_SECS;
    app.health_check_timeout_secs = HEALTH_TIMEOUT_SECS;
    app.health_check_retries = 1;
    app.restart_max_retries = 0;
    app.error_control = ErrorControl::Critical;

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8000, 50)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch");

    (
        supervisor,
        APP_LAUNCH_NS + HEALTH_INTERVAL_SECS * 1_000_000_000,
    )
}

pub(super) fn launch_due_health_check(
    supervisor: &mut Supervisor,
    due_at_ns: u64,
    pid: u32,
    pidfd: i32,
) -> SupervisorHealthCheckLaunchDispatch {
    supervisor
        .process_due_health_check_intervals(due_at_ns)
        .expect("health interval");
    launch_next_health_check(supervisor, due_at_ns + 10_000, pid, pidfd)
}

pub(super) fn launch_next_health_check(
    supervisor: &mut Supervisor,
    launched_at_ns: u64,
    pid: u32,
    pidfd: i32,
) -> SupervisorHealthCheckLaunchDispatch {
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(pid, pidfd)]);
    let mut clock = ScriptedClock::new([launched_at_ns]);
    let mut controller = TestProcessController::default();
    let result = supervisor
        .launch_next_pending_health_check_job(
            &mut tokens,
            &mut launcher,
            &mut clock,
            &mut controller,
        )
        .expect("launch health")
        .expect("health launch result");
    let SupervisorHealthCheckLaunchResult::Launched(dispatch) = result else {
        panic!("expected health launch");
    };
    dispatch
}

pub(super) struct NoopFinalizer;

impl ShutdownFinalizer for NoopFinalizer {
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

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct RecordingFinalizer {
    pub(super) calls: Vec<FinalizerCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FinalizerCall {
    Sync,
    Reboot,
}

impl ShutdownFinalizer for RecordingFinalizer {
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
        self.calls.push(FinalizerCall::Sync);
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        assert_eq!(kind, ShutdownKind::Reboot);
        self.calls.push(FinalizerCall::Reboot);
        Ok(())
    }
}
