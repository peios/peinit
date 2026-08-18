use crate::boundary::{ChildExitStatus, ChildReap, LinuxSignalFdRead};
use crate::runtime::RuntimeEventSource;
use crate::service::runtime::ServiceState;
use crate::supervisor::tests::{alive_service, process};

use super::support::{
    FakeChildReaper, FakeSignalSource, LoopScript, MAIN_LAUNCH_NS, PRE_HOOK_DONE_NS,
    PRE_HOOK_LAUNCH_NS, boot_supervisor, run_loop,
};

#[test]
fn runtime_pump_launches_main_after_pre_start_hook_sigchld() {
    let mut app = alive_service("app");
    app.exec_start_pre = vec!["/bin/pre".to_string()];
    app.start_timeout_secs = 45;
    let mut supervisor = boot_supervisor(vec![app]);

    let result = run_loop(
        &mut supervisor,
        LoopScript::new(
            [PRE_HOOK_LAUNCH_NS, PRE_HOOK_DONE_NS, MAIN_LAUNCH_NS],
            vec![process(6100, 71), process(6200, 72)],
        )
        .events([RuntimeEventSource::Pid1Signal])
        .signal(FakeSignalSource::new([LinuxSignalFdRead::Other {
            signal: libc::SIGCHLD,
        }]))
        .child_reaper(FakeChildReaper::new([Ok(vec![ChildReap {
            pid: 6100,
            status: ChildExitStatus::Exited { code: 0 },
        }])])),
    );

    assert_eq!(result.turn.pre_work.start_hook_launches.len(), 1);
    assert_eq!(result.turn.post_work.service_launches.len(), 1);
    assert_eq!(
        result.controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/hooks".to_string()],
    );
    assert_eq!(result.token_jobs, vec!["app", "app"]);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    assert!(supervisor.pending_start_hook_launch_jobs().is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
}
