use crate::boundary::{ChildExitStatus, ChildReap, LaunchedProcess, LinuxSignalFdRead};
use crate::execution::job_started::ServiceMainJobStartedDispatch;
use crate::execution::job_terminal::ServiceMainJobTerminalDispatch;
use crate::execution::launch::LaunchCreatedJobDispatch;
use crate::ids::{JobId, JobIdAllocator};
use crate::job::{JobEvent, JobEventDetail, JobState, JobType};
use crate::runtime::{RuntimeLogPipeTurn, RuntimeShutdownEventTurn, RuntimeWorkPumpTurn};
use crate::security::TokenSummary;
use crate::service::ServiceTableTransition;
use crate::service::runtime::{ServiceState, ServiceTransitionEvent, TransitionCause};
use crate::shutdown::{
    CleanupActionResult, ShutdownFinalizationReport, ShutdownFinalizationState, ShutdownKind,
    ShutdownPlan, ShutdownRuntime,
};
use crate::supervisor::{
    SupervisorChildReapDispatch, SupervisorChildReapTurn, SupervisorLaunchDispatch,
    SupervisorPid1SignalFdTurn, SupervisorShutdownDispatch, SupervisorShutdownFinalizationDispatch,
    SupervisorShutdownSignalAction, SupervisorShutdownSignalDispatch,
    SupervisorShutdownStopDispatch, SupervisorTerminalDispatch,
};

use super::collect_runtime_loop_console_messages;

#[test]
fn a_reset_that_leaves_the_abandoned_cgroup_leaked_warns_on_the_console() {
    let dispatch = crate::supervisor::SupervisorLifecycleDispatch {
        outcome: crate::control::lifecycle::LifecycleCommandOutcome::Noop(
            crate::control::lifecycle::ServiceStatusSnapshot {
                service: "task".to_string(),
                state: ServiceState::Inactive,
                cause: None,
                generation: 1,
                definition_removed: false,
            },
        ),
        context_id: None,
        pending_control_operation: None,
        lifecycle_warnings: vec![
            "abandoned main cgroup for service task is still populated after reset -- cgroup remains leaked; underlying D-state process requires investigation"
                .to_string(),
        ],
        start_dispatches: Vec::new(),
    };
    let turn = RuntimeShutdownEventTurn::ControlConnection {
        fd: 44,
        supervisor: Box::new(crate::supervisor::SupervisorControlConnectionTableTurn {
            fd: 44,
            turn: crate::supervisor::SupervisorControlConnectionTurn {
                read: crate::control::connection::ControlConnectionReadTurn::WouldBlock {
                    buffered_bytes: 0,
                },
                frame: Some(crate::supervisor::SupervisorControlConnectionFrameTurn {
                    frame: crate::supervisor::SupervisorControlFrameTurn::CommandAccepted {
                        response_line: None,
                        dispatch: Some(Box::new(
                            crate::supervisor::SupervisorControlCommandDispatch::Lifecycle(
                                Box::new(dispatch),
                            ),
                        )),
                        wait: None,
                        access_denials: Vec::new(),
                        job_access_denials: Vec::new(),
                        remaining_bytes: 0,
                    },
                    pending_write_bytes: 0,
                    close_after_write: false,
                }),
                write: crate::control::connection::ControlConnectionWriteTurn::Idle {
                    close_after_write: false,
                },
                close_connection: false,
            },
            removed: false,
            active_connections: 1,
        }),
        deadline_timer: None,
    };

    let mut messages = Vec::new();
    collect_runtime_loop_console_messages(
        &RuntimeWorkPumpTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
        &[],
        &mut messages,
    );

    assert_eq!(
        messages,
        vec![crate::runtime::console::ConsoleMessage::error(
            "peinit warning: abandoned main cgroup for service task is still populated after reset -- cgroup remains leaked; underlying D-state process requires investigation\n"
        )],
    );
}

// PEI-359. §5.3 asks for a warning when a service announces RELOADING=1 and
// never completes the reload. That string existed only as an *operation
// result*, returned to a `wait=true` caller — and `reload` defaults to
// `wait=false`, so the default way to issue one produced no record anywhere.
// The console handled `reload_command_timeouts` and never `reload_detections`.
#[test]
fn an_unconfirmed_reload_is_reported_on_the_console() {
    let turn = reload_detection_turn(
        crate::execution::control::ReloadDetectionPhase::ExtendedWait,
    );

    let mut messages = Vec::new();
    collect_runtime_loop_console_messages(
        &RuntimeWorkPumpTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
        &[],
        &mut messages,
    );

    assert_eq!(
        messages,
        vec![crate::runtime::console::ConsoleMessage::error(
            "peinit: service app signalled RELOADING=1 but never completed reload\n"
        )],
    );
}

// The ordinary outcome stays quiet. A service that never implements the
// handshake lets its detection window expire on every single reload; reporting
// that would make the line above worthless.
#[test]
fn an_expired_reload_detection_window_says_nothing() {
    let turn = reload_detection_turn(
        crate::execution::control::ReloadDetectionPhase::DetectionWindow,
    );

    let mut messages = Vec::new();
    collect_runtime_loop_console_messages(
        &RuntimeWorkPumpTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
        &[],
        &mut messages,
    );

    assert!(messages.is_empty());
}

fn reload_detection_turn(
    phase: crate::execution::control::ReloadDetectionPhase,
) -> RuntimeShutdownEventTurn {
    RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        read: crate::boundary::LinuxTimerFdRead::Expired { expirations: 1 },
        drive: Some(Box::new(
            crate::supervisor::SupervisorLifecycleDeadlineDispatch {
                reload_detections: vec![crate::supervisor::SupervisorReloadDetectionDispatch {
                    completion: crate::execution::control::ReloadDetectionCompletion {
                        operation_event: crate::operation::store::OperationEvent {
                            operation_id: operation_id(),
                            operation_type: crate::operation::OperationType::Reload,
                            service: "app".to_string(),
                            source: crate::operation::OperationSource::Admin,
                            caller: None,
                            state: crate::operation::OperationState::Completed,
                            detail: crate::operation::store::OperationEventDetail::Completed {
                                duration_ns: 1_000,
                                result: "reload signal advisory".to_string(),
                            },
                        },
                        service_transition: transition(
                            "app",
                            ServiceState::Reloading,
                            ServiceState::Active,
                            TransitionCause::ExplicitReload,
                        ),
                        phase,
                    },
                }],
                ..crate::supervisor::SupervisorLifecycleDeadlineDispatch::default()
            },
        )),
        deadline_timer: crate::supervisor::SupervisorLifecycleDeadlineTimerTurn::Disarmed,
    }
}

#[test]
fn a_leaked_cgroup_is_announced_on_the_console() {
    let turn = RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        read: crate::boundary::LinuxTimerFdRead::Expired { expirations: 1 },
        drive: Some(Box::new(
            crate::supervisor::SupervisorLifecycleDeadlineDispatch {
                cgroup_leaks: vec![crate::supervisor::SupervisorLeakedCgroupDispatch {
                    service: "jellyfin".to_string(),
                    path: "/sys/fs/cgroup/peinit/jellyfin/health".to_string(),
                    kind: crate::service::runtime::LeakedCgroupKind::Health,
                    detected_at_ns: 1_000,
                }],
                ..crate::supervisor::SupervisorLifecycleDeadlineDispatch::default()
            },
        )),
        deadline_timer: crate::supervisor::SupervisorLifecycleDeadlineTimerTurn::Disarmed,
    };

    let mut messages = Vec::new();
    collect_runtime_loop_console_messages(
        &RuntimeWorkPumpTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
        &[],
        &mut messages,
    );

    assert_eq!(
        messages,
        vec![crate::runtime::console::ConsoleMessage::error(
            "peinit: service jellyfin leaked its health cgroup /sys/fs/cgroup/peinit/jellyfin/health; underlying process is not responding to the kernel\n"
        )],
    );
}

#[test]
fn service_launch_emits_console_progress() {
    let work = RuntimeWorkPumpTurn {
        service_launches: vec![SupervisorLaunchDispatch {
            launch: LaunchCreatedJobDispatch {
                job_id: job_id(1),
                process: LaunchedProcess {
                    pid: 700,
                    pidfd: 70,
                    stdout_fd: None,
                    stderr_fd: None,
                    setup_status_fd: None,
                    cleanup_evidence: Vec::new(),
                },
                job_event: created_job("app", job_id(1)),
            },
            started: ServiceMainJobStartedDispatch {
                job_event: running_job("app", job_id(1)),
                operation_events: Vec::new(),
                service_transitions: Vec::new(),
                graph_events: Vec::new(),
                post_start_hook: None,
            },
            start_dispatches: Vec::new(),
        }],
        ..RuntimeWorkPumpTurn::default()
    };

    let messages = collect_messages(&work, &[], &RuntimeWorkPumpTurn::default());

    assert_eq!(messages, vec!["peinit: service app started\n"]);
}

#[test]
fn service_log_pipe_turn_does_not_echo_output_to_console() {
    let turn = RuntimeShutdownEventTurn::ServiceLogPipe {
        pipe: RuntimeLogPipeTurn::Read {
            fd: 9,
            records: Vec::new(),
            closed: false,
            would_block: false,
            buffered_records: 0,
            output_dropped: None,
        },
    };

    let messages = collect_messages(
        &RuntimeWorkPumpTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
    );

    assert!(messages.is_empty());
}

#[test]
fn critical_service_failure_emits_failure_and_critical_messages() {
    let service = "database";
    let job_id = job_id(2);
    let turn = RuntimeShutdownEventTurn::Pid1Signal {
        read: LinuxSignalFdRead::Other {
            signal: libc::SIGCHLD,
        },
        supervisor: SupervisorPid1SignalFdTurn::Other {
            signal: libc::SIGCHLD,
        },
        child_reaps: vec![SupervisorChildReapTurn::Tracked {
            child: ChildReap {
                pid: 800,
                status: ChildExitStatus::Exited { code: 1 },
            },
            job_id,
            dispatch: SupervisorChildReapDispatch::Runtime(Box::new(SupervisorTerminalDispatch {
                terminal: ServiceMainJobTerminalDispatch {
                    job_event: failed_job(service, job_id),
                    operation_events: Vec::new(),
                    service_transitions: vec![transition(
                        service,
                        ServiceState::Active,
                        ServiceState::Failed,
                        TransitionCause::ProcessCrash,
                    )],
                    graph_events: Vec::new(),
                    post_start_hook: None,
                    late_exit: None,
                },
                cleanup_job_events: Vec::new(),
                start_dispatches: Vec::new(),
                restart_start_dispatches: Vec::new(),
                critical_reboot: Some(finalization_dispatch()),
            })),
        }],
        drive: None,
        deadline_timer: None,
    };

    let messages = collect_messages(
        &RuntimeWorkPumpTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
    );

    assert_eq!(
        messages,
        vec![
            "peinit: service database failed: ProcessCrash\n",
            "peinit: critical service database failed: service main exited\n",
        ],
    );
}

// PEI-531. A late exit produces no service transition — that is the whole
// point of it — so without a message of its own it leaves no trace at all, and
// a service sitting in a stale state becomes a silent mystery. Every route to
// one is something having gone wrong earlier, so it is an error rather than
// status and survives a requested blackout.
#[test]
fn a_late_service_exit_is_reported_to_the_console() {
    let service = "atriumd";
    let job_id = job_id(3);
    let turn = RuntimeShutdownEventTurn::Pid1Signal {
        read: LinuxSignalFdRead::Other {
            signal: libc::SIGCHLD,
        },
        supervisor: SupervisorPid1SignalFdTurn::Other {
            signal: libc::SIGCHLD,
        },
        child_reaps: vec![SupervisorChildReapTurn::Tracked {
            child: ChildReap {
                pid: 801,
                status: ChildExitStatus::Exited { code: 0 },
            },
            job_id,
            dispatch: SupervisorChildReapDispatch::Runtime(Box::new(SupervisorTerminalDispatch {
                terminal: ServiceMainJobTerminalDispatch {
                    job_event: failed_job(service, job_id),
                    operation_events: Vec::new(),
                    service_transitions: Vec::new(),
                    graph_events: Vec::new(),
                    post_start_hook: None,
                    late_exit: Some(ServiceState::Backoff),
                },
                cleanup_job_events: Vec::new(),
                start_dispatches: Vec::new(),
                restart_start_dispatches: Vec::new(),
                critical_reboot: None,
            })),
        }],
        drive: None,
        deadline_timer: None,
    };

    let messages = collect_messages(
        &RuntimeWorkPumpTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
    );

    assert_eq!(
        messages,
        vec!["peinit: service atriumd main process exited in state Backoff; no action taken\n"],
    );
}

#[test]
fn shutdown_signal_emits_shutdown_progress() {
    let turn = RuntimeShutdownEventTurn::Pid1Signal {
        read: LinuxSignalFdRead::Shutdown(crate::shutdown::ShutdownSignal::Sigterm),
        supervisor: SupervisorPid1SignalFdTurn::Shutdown(Box::new(
            SupervisorShutdownSignalDispatch {
                signal: crate::shutdown::ShutdownSignal::Sigterm,
                action: SupervisorShutdownSignalAction::Graceful(SupervisorShutdownDispatch {
                    submitted_stops: Vec::new(),
                    runtime: shutdown_runtime(ShutdownKind::Poweroff),
                    completed_transitions: Vec::new(),
                    killed_starting: Vec::new(),
                    first_wave: vec![SupervisorShutdownStopDispatch {
                        service: "app".to_string(),
                        already_stopping: false,
                        target: None,
                        signal: None,
                        service_transition: None,
                        deadline: None,
                        unsubstantiated_deadline: None,
                    }],
                    startup_operation_events: Vec::new(),
                    startup_job_events: Vec::new(),
                }),
            },
        )),
        child_reaps: Vec::new(),
        drive: None,
        deadline_timer: None,
    };

    let messages = collect_messages(
        &RuntimeWorkPumpTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
    );

    assert_eq!(
        messages,
        vec![
            "peinit: shutdown Poweroff started\n",
            "peinit: shutdown stopping app\n",
        ],
    );
}

fn collect_messages(
    pre_work: &RuntimeWorkPumpTurn,
    turns: &[RuntimeShutdownEventTurn],
    post_work: &RuntimeWorkPumpTurn,
) -> Vec<String> {
    let mut messages = Vec::new();
    collect_runtime_loop_console_messages(pre_work, turns, post_work, &[], &mut messages);
    messages.into_iter().map(|message| message.text).collect()
}

/// A launch failure must say WHY on the console. The cause is already on the
/// job event — a token that could not be materialised, a runtime directory that
/// could not be secured — and printing only the service name leaves an operator
/// unable to tell those apart.
#[test]
fn launch_failure_console_message_carries_the_cause() {
    let mut out = Vec::new();
    let mut event = created_job("lpsd", job_id(1));
    event.failure_cause = Some("provision runtime directories for lpsd failed".to_string());

    super::push_service_launch_failed(&mut out, &event);

    assert_eq!(
        out,
        vec![super::ConsoleMessage::error(
            "peinit: service lpsd failed to launch: provision runtime directories for lpsd failed\n"
        )],
    );
}

/// A cause is not guaranteed to be present, and its absence must not swallow
/// the report of the failure itself.
#[test]
fn launch_failure_console_message_without_a_cause_still_reports_the_failure() {
    let mut out = Vec::new();
    let mut event = created_job("lpsd", job_id(1));
    event.failure_cause = None;

    super::push_service_launch_failed(&mut out, &event);

    // Error, not Status: a launch failure has to survive `peios.quiet=2`.
    assert_eq!(
        out,
        vec![super::ConsoleMessage::error(
            "peinit: service lpsd failed to launch\n"
        )],
    );
}

fn created_job(service: &str, id: JobId) -> JobEvent {
    job_event(
        service,
        id,
        JobState::Created,
        None,
        None,
        None,
        JobEventDetail::Created {
            image_path: format!("/sbin/{service}"),
            identity: "SYSTEM".to_string(),
            operation_id: None,
        },
    )
}

fn running_job(service: &str, id: JobId) -> JobEvent {
    job_event(
        service,
        id,
        JobState::Running,
        Some(700),
        Some(70),
        None,
        JobEventDetail::Started {
            started_at_ns: 20,
            pid: 700,
            cgroup_id: service.to_string(),
        },
    )
}

fn failed_job(service: &str, id: JobId) -> JobEvent {
    job_event(
        service,
        id,
        JobState::Failed,
        Some(800),
        Some(80),
        Some(30),
        JobEventDetail::Ended {
            ended_at_ns: 30,
            duration_ns: 10,
            exit_code: Some(1),
            exit_signal: None,
            failure_cause: Some("exit 1".to_string()),
        },
    )
}

fn job_event(
    service: &str,
    id: JobId,
    state: JobState,
    pid: Option<u32>,
    pidfd: Option<i32>,
    ended_at_ns: Option<u64>,
    detail: JobEventDetail,
) -> JobEvent {
    JobEvent {
        job_id: id,
        service: Some(service.to_string()),
        job_type: JobType::ServiceMain,
        hook_index: None,
        state,
        pid,
        pidfd,
        resolved_identity: "SYSTEM".to_string(),
        operation_id: None,
        token_summary: TokenSummary::requested_identity("SYSTEM"),
        image_path: format!("/sbin/{service}"),
        arguments: Vec::new(),
        created_at_ns: 10,
        started_at_ns: pid.map(|_| 20),
        ended_at_ns,
        exit_code: if state == JobState::Failed {
            Some(1)
        } else {
            None
        },
        exit_signal: None,
        failure_cause: if state == JobState::Failed {
            Some("exit 1".to_string())
        } else {
            None
        },
        cgroup_id: service.to_string(),
        activation_generation: 1,
        cgroup_generation: 1,
        detail,
    }
}

fn transition(
    service: &str,
    from: ServiceState,
    to: ServiceState,
    cause: TransitionCause,
) -> ServiceTableTransition {
    ServiceTableTransition {
        event: ServiceTransitionEvent {
            service: service.to_string(),
            from,
            to,
            cause,
            generation: 1,
        },
        discarded_definition_removed: false,
    }
}

fn shutdown_runtime(kind: ShutdownKind) -> ShutdownRuntime {
    ShutdownRuntime {
        kind,
        initiated_at_ns: 1,
        global_deadline_ns: 90,
        plan: ShutdownPlan {
            completed_to_clear: Vec::new(),
            starting_to_kill: Vec::new(),
            stop_waves: Vec::new(),
            ignored: Vec::new(),
        },
        current_wave: 0,
        stop_deadlines: Vec::new(),
        post_kill_deadlines: Vec::new(),
        finalization: ShutdownFinalizationState::WaitingForServices,
    }
}

fn finalization_dispatch() -> SupervisorShutdownFinalizationDispatch {
    SupervisorShutdownFinalizationDispatch {
        report: ShutdownFinalizationReport {
            random_seed: CleanupActionResult::Ok,
            snapshot_mounts: CleanupActionResult::Ok,
            mount_results: Vec::new(),
            root_remount: CleanupActionResult::Ok,
            sync_result: CleanupActionResult::Ok,
            reboot_result: CleanupActionResult::Ok,
        },
        finalization: ShutdownFinalizationState::Completed,
    }
}

fn operation_id() -> crate::ids::OperationId {
    crate::ids::OperationIdAllocator::new()
        .allocate_batch(1, 1_717_171_717_123_456_789)
        .expect("operation id")[0]
}

fn job_id(sequence: u64) -> JobId {
    let mut allocator = JobIdAllocator::new();
    allocator
        .allocate_batch(sequence as usize + 1, 1_717_171_717_123_456_789)
        .expect("job id")[sequence as usize]
}
