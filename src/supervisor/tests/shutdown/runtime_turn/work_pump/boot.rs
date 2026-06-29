use std::io::Write;
use std::os::fd::IntoRawFd;

use crate::boundary::{
    BoundaryError, FilesystemCheckReport, FilesystemCheckResult, LaunchedProcess,
};
use crate::runtime::RuntimeEventSource;
use crate::runtime::{
    RuntimeEventdLogFlush, RuntimeFilesystemCheckHelperTurn, RuntimeLogPipeTurn,
    RuntimeShutdownEventTurn,
};
use crate::service::runtime::ServiceState;
use crate::service::{ServiceCheck, ServiceCheckKind, ServiceDefinition};
use crate::supervisor::tests::{APP_LAUNCH_NS, AUTHD_LAUNCH_NS, alive_service, process};

use super::support::{
    DEPENDENT_LAUNCH_NS, FakeDeadlineTimer, FakeNotifySource, LoopScript, NOTIFY_NS,
    boot_supervisor, boot_supervisor_with_eventd_log_socket_path, datagram, pipe_pair, run_loop,
};

const FILESYSTEM_CHECK_NS: u64 = NOTIFY_NS + 1_000;
const FILESYSTEM_CHECK_TIMEOUT_NS: u64 = FILESYSTEM_CHECK_NS + 5_000_000_000;

#[test]
fn runtime_pump_launches_boot_dependency_chain_before_waiting() {
    let mut app = alive_service("app");
    app.requires.push("authd".to_string());
    let mut authd = alive_service("authd");
    authd.triggers.clear();
    let mut supervisor = boot_supervisor(vec![app, authd]);

    let result = run_loop(
        &mut supervisor,
        LoopScript::new(
            [AUTHD_LAUNCH_NS, APP_LAUNCH_NS],
            vec![process(4242, 9), process(4243, 10)],
        ),
    );

    assert!(result.turn.sources.is_empty());
    assert_eq!(result.turn.pre_work.service_launches.len(), 2);
    assert!(result.turn.post_work.is_empty());
    assert_eq!(result.token_jobs, vec!["authd", "app"]);
    assert_eq!(result.launched_jobs, vec!["authd", "app"]);
    assert_eq!(
        supervisor.service_status("authd").expect("authd").state,
        ServiceState::Active,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    assert!(supervisor.pending_launch_jobs().is_empty());
}

#[test]
fn runtime_pump_launches_and_registers_filesystem_condition_helper_before_waiting() {
    let (app, check) = filesystem_condition_app();
    let mut supervisor = boot_supervisor(vec![app]);
    let operation_id = supervisor.pending_pre_start_check_launches()[0];

    let result = run_loop(
        &mut supervisor,
        LoopScript::new([FILESYSTEM_CHECK_NS], Vec::new()),
    );

    assert!(result.turn.sources.is_empty());
    assert_eq!(result.turn.pre_work.filesystem_check_launches.len(), 1);
    assert!(result.turn.pre_work.service_launches.is_empty());
    assert_eq!(result.filesystem_check_requests.len(), 1);
    assert_eq!(
        result.filesystem_check_requests[0].operation_id,
        operation_id
    );
    assert_eq!(result.filesystem_check_requests[0].checks, vec![check]);
    assert_eq!(
        result
            .registrar_calls
            .iter()
            .map(|call| call.source)
            .collect::<Vec<_>>(),
        vec![
            RuntimeEventSource::FilesystemCheckHelper { result_fd: 81 },
            RuntimeEventSource::FilesystemCheckHelperExit { pidfd: 80 },
        ],
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert!(supervisor.pending_pre_start_check_launches().is_empty());
}

#[test]
fn runtime_filesystem_condition_completion_queues_and_launches_service_job() {
    let (app, check) = filesystem_condition_app();
    let mut supervisor = boot_supervisor(vec![app]);
    let operation_id = supervisor.pending_pre_start_check_launches()[0];

    run_loop(
        &mut supervisor,
        LoopScript::new([FILESYSTEM_CHECK_NS], Vec::new()),
    );

    let result = run_loop(
        &mut supervisor,
        LoopScript::new(
            [FILESYSTEM_CHECK_NS, FILESYSTEM_CHECK_NS + 1],
            vec![process(4242, 9)],
        )
        .events([RuntimeEventSource::FilesystemCheckHelper { result_fd: 81 }])
        .filesystem_reports([Ok(Some(FilesystemCheckReport {
            service: "app".to_string(),
            operation_id,
            results: vec![FilesystemCheckResult {
                check,
                satisfied: true,
            }],
        }))]),
    );

    assert_eq!(
        result.turn.sources,
        vec![RuntimeEventSource::FilesystemCheckHelper { result_fd: 81 }],
    );
    assert!(matches!(
        result.turn.turns.as_slice(),
        [RuntimeShutdownEventTurn::FilesystemCheckHelper {
            result_fd: 81,
            turn: RuntimeFilesystemCheckHelperTurn::Completed { .. },
        }],
    ));
    assert_eq!(result.filesystem_check_reader_helpers.len(), 1);
    assert_eq!(result.turn.post_work.service_launches.len(), 1);
    assert_eq!(result.launched_jobs, vec!["app"]);
    assert_eq!(result.registrar_unregister_calls, vec![81, 80]);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
}

#[test]
fn runtime_filesystem_condition_read_failure_skips_service_fail_closed() {
    let (app, _check) = filesystem_condition_app();
    let mut supervisor = boot_supervisor(vec![app]);

    run_loop(
        &mut supervisor,
        LoopScript::new([FILESYSTEM_CHECK_NS], Vec::new()),
    );

    let result = run_loop(
        &mut supervisor,
        LoopScript::new([FILESYSTEM_CHECK_NS], Vec::new())
            .events([RuntimeEventSource::FilesystemCheckHelper { result_fd: 81 }])
            .filesystem_reports([Err(BoundaryError::Process("read failed".to_string()))]),
    );

    assert!(matches!(
        result.turn.turns.as_slice(),
        [RuntimeShutdownEventTurn::FilesystemCheckHelper {
            result_fd: 81,
            turn: RuntimeFilesystemCheckHelperTurn::ReadFailedClosed { .. },
        }],
    ));
    assert!(result.turn.post_work.is_empty());
    assert_eq!(result.registrar_unregister_calls, vec![81, 80]);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Skipped,
    );
    assert!(supervisor.pending_launch_jobs().is_empty());
}

#[test]
fn runtime_lifecycle_timeout_kills_helper_and_skips_filesystem_condition() {
    let (app, _check) = filesystem_condition_app();
    let mut supervisor = boot_supervisor(vec![app]);

    run_loop(
        &mut supervisor,
        LoopScript::new([FILESYSTEM_CHECK_NS], Vec::new()),
    );
    let deadline = supervisor
        .next_pre_start_check_timeout()
        .expect("pre-start check timeout");
    assert_eq!(deadline.due_at_ns, FILESYSTEM_CHECK_TIMEOUT_NS);

    let result = run_loop(
        &mut supervisor,
        LoopScript::new([FILESYSTEM_CHECK_TIMEOUT_NS], Vec::new())
            .events([RuntimeEventSource::LifecycleDeadlineTimer])
            .lifecycle_timer(FakeDeadlineTimer::expired_once()),
    );

    assert!(matches!(
        result.turn.turns.as_slice(),
        [RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
            drive: Some(drive),
            ..
        }] if drive.pre_start_check_timeouts.len() == 1
    ));
    assert_eq!(
        result.controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/app/checks".to_string()],
    );
    assert_eq!(result.registrar_unregister_calls, vec![81, 80]);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Skipped,
    );
    assert!(supervisor.next_pre_start_check_timeout().is_none());
    assert!(supervisor.pending_launch_jobs().is_empty());
}

#[test]
fn runtime_pidfd_exit_without_report_skips_filesystem_condition_fail_closed() {
    let (app, _check) = filesystem_condition_app();
    let mut supervisor = boot_supervisor(vec![app]);

    run_loop(
        &mut supervisor,
        LoopScript::new([FILESYSTEM_CHECK_NS], Vec::new()),
    );

    let result = run_loop(
        &mut supervisor,
        LoopScript::new([FILESYSTEM_CHECK_NS + 1], Vec::new())
            .events([RuntimeEventSource::FilesystemCheckHelperExit { pidfd: 80 }])
            .filesystem_reports([Ok(None)]),
    );

    assert!(matches!(
        result.turn.turns.as_slice(),
        [RuntimeShutdownEventTurn::FilesystemCheckHelperExit {
            pidfd: 80,
            turn: RuntimeFilesystemCheckHelperTurn::ReadFailedClosed { .. },
        }],
    ));
    assert_eq!(result.registrar_unregister_calls, vec![81, 80]);
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Skipped,
    );
    assert!(supervisor.next_pre_start_check_timeout().is_none());
}

#[test]
fn runtime_pump_registers_new_launch_log_pipes_before_waiting() {
    let app = alive_service("app");
    let mut supervisor = boot_supervisor(vec![app]);
    let (stdout_read, _stdout_write) = pipe_pair();
    let (stderr_read, _stderr_write) = pipe_pair();
    let stdout_fd = stdout_read.into_raw_fd();
    let stderr_fd = stderr_read.into_raw_fd();

    let result = run_loop(
        &mut supervisor,
        LoopScript::new(
            [APP_LAUNCH_NS],
            vec![LaunchedProcess {
                pid: 4242,
                pidfd: 9,
                stdout_fd: Some(stdout_fd),
                stderr_fd: Some(stderr_fd),
                setup_status_fd: None,
                cleanup_evidence: Vec::new(),
            }],
        ),
    );

    assert_eq!(result.turn.pre_work.service_launches.len(), 1);
    assert!(result.turn.sources.is_empty());
    assert_eq!(result.active_log_pipe_count, 2);
    assert_eq!(
        result
            .registrar_calls
            .iter()
            .map(|call| call.source)
            .collect::<Vec<_>>(),
        vec![
            RuntimeEventSource::ServiceLogPipe { fd: stdout_fd },
            RuntimeEventSource::ServiceLogPipe { fd: stderr_fd },
        ],
    );
    assert!(result.buffered_logs.is_empty());
    assert_eq!(result.turn.eventd_flush, RuntimeEventdLogFlush::default());
}

fn filesystem_condition_app() -> (ServiceDefinition, ServiceCheck) {
    let mut app = alive_service("app");
    let check = ServiceCheck {
        kind: ServiceCheckKind::Path,
        argument: "/srv/app".to_string(),
    };
    app.conditions = vec![check.clone()];
    (app, check)
}

#[test]
fn runtime_loop_buffers_service_logs_until_eventd_socket_is_configured() {
    let app = alive_service("app");
    let mut supervisor = boot_supervisor(vec![app]);
    let (stdout_read, mut stdout_write) = pipe_pair();
    let stdout_fd = stdout_read.into_raw_fd();
    stdout_write
        .write_all(b"runtime ready\n")
        .expect("write service log");
    drop(stdout_write);

    let result = run_loop(
        &mut supervisor,
        LoopScript::new(
            [APP_LAUNCH_NS],
            vec![LaunchedProcess {
                pid: 4242,
                pidfd: 9,
                stdout_fd: Some(stdout_fd),
                stderr_fd: None,
                setup_status_fd: None,
                cleanup_evidence: Vec::new(),
            }],
        )
        .events([RuntimeEventSource::ServiceLogPipe { fd: stdout_fd }]),
    );

    assert_eq!(
        result
            .buffered_logs
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["runtime ready"],
    );
    assert_eq!(result.active_log_pipe_count, 0);
    assert_eq!(result.registrar_unregister_calls, vec![stdout_fd]);
    assert_eq!(
        result.turn.eventd_flush,
        RuntimeEventdLogFlush::not_configured(1)
    );
    assert!(matches!(
        result.turn.turns.as_slice(),
        [RuntimeShutdownEventTurn::ServiceLogPipe {
            pipe: RuntimeLogPipeTurn::Read {
                fd,
                closed: true,
                would_block: false,
                buffered_records: 1,
                ..
            }
        }] if *fd == stdout_fd
    ));
}

#[test]
fn runtime_loop_buffers_service_logs_until_eventd_is_active() {
    let app = alive_service("app");
    let mut supervisor =
        boot_supervisor_with_eventd_log_socket_path(vec![app], "/run/peios/eventd-log.sock");
    let (stdout_read, mut stdout_write) = pipe_pair();
    let stdout_fd = stdout_read.into_raw_fd();
    stdout_write
        .write_all(b"runtime ready\n")
        .expect("write service log");
    drop(stdout_write);

    let result = run_loop(
        &mut supervisor,
        LoopScript::new(
            [APP_LAUNCH_NS],
            vec![LaunchedProcess {
                pid: 4242,
                pidfd: 9,
                stdout_fd: Some(stdout_fd),
                stderr_fd: None,
                setup_status_fd: None,
                cleanup_evidence: Vec::new(),
            }],
        )
        .events([RuntimeEventSource::ServiceLogPipe { fd: stdout_fd }]),
    );

    assert_eq!(
        result
            .buffered_logs
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["runtime ready"],
    );
    assert_eq!(
        result.turn.eventd_flush,
        RuntimeEventdLogFlush::unavailable(false, true, 1)
    );
}

#[test]
fn runtime_pump_launches_dependent_after_ready_notify() {
    let mut app = alive_service("app");
    app.requires.push("authd".to_string());
    let mut authd = ServiceDefinition::simple_system_boot("authd", "/sbin/authd");
    authd.triggers.clear();
    let mut supervisor = boot_supervisor(vec![app, authd]);

    let result = run_loop(
        &mut supervisor,
        LoopScript::new(
            [AUTHD_LAUNCH_NS, NOTIFY_NS, DEPENDENT_LAUNCH_NS],
            vec![process(4242, 9), process(4243, 10)],
        )
        .events([RuntimeEventSource::NotifySocket])
        .notify(FakeNotifySource::new([Ok(Some(datagram(
            4242,
            b"STATUS=Listening\nREADY=1",
        )))])),
    );

    assert_eq!(result.turn.sources, vec![RuntimeEventSource::NotifySocket]);
    assert_eq!(result.turn.pre_work.service_launches.len(), 1);
    assert_eq!(result.turn.post_work.service_launches.len(), 1);
    assert_eq!(result.token_jobs, vec!["authd", "app"]);
    assert_eq!(result.launched_jobs, vec!["authd", "app"]);
    assert_eq!(
        supervisor.service_status("authd").expect("authd").state,
        ServiceState::Active,
    );
    assert_eq!(
        supervisor
            .service_status("authd")
            .expect("authd")
            .status_text
            .as_deref(),
        Some("Listening"),
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Active,
    );
    assert!(supervisor.pending_launch_jobs().is_empty());
}
