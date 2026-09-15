use crate::boundary::{KmesEvent, LinuxTimerFdRead};
use crate::control::connection::{ControlConnectionReadTurn, ControlConnectionWriteTurn};
use crate::control::lifecycle::{OnDemandStartDispatch, OnDemandStartPlan};
use crate::control::service_security::{ServiceAccess, ServiceAccessDenied};
use crate::execution::graph::GraphContextId;
use crate::ids::OperationIdAllocator;
use crate::operation::conflict::OperationConflictDecision;
use crate::operation::store::{OperationEvent, OperationRequestOutcome};
use crate::operation::{OperationRecord, OperationSource, OperationState, OperationType};
use crate::runtime::{RuntimeCalendarTimerTurn, RuntimeShutdownEventTurn, RuntimeWorkPumpTurn};
use crate::security::TokenSummary;
use crate::service::ServiceTableTransition;
use crate::service::runtime::{ServiceState, ServiceTransitionEvent, TransitionCause};
use crate::shutdown::ShutdownFinalizationState;
use crate::supervisor::{
    SupervisorControlCommandBodyError, SupervisorControlConnectionFrameTurn,
    SupervisorControlConnectionTableTurn, SupervisorControlConnectionTurn,
    SupervisorControlFrameTurn, SupervisorOnFailureLoopSuppressedDispatch,
    SupervisorOnFailureLoopSuppressionReason, SupervisorOperationMaintenanceTurn,
    SupervisorShutdownAbandonedDispatch, SupervisorShutdownDeadlineTimerTurn,
    SupervisorShutdownDriveDispatch, SupervisorShutdownTimeoutDispatch, SupervisorTimerAction,
    SupervisorTimerDispatch,
};

use super::collect_runtime_loop_kmes_events;

#[test]
fn runtime_loop_collector_pushes_a_leaked_cgroup_as_an_audit_event() {
    let turn = RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        read: LinuxTimerFdRead::Expired { expirations: 1 },
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

    let mut events = Vec::new();
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
        &mut events,
    )
    .expect("collected KMES events");

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["cgroup.leaked"],
    );
}

#[test]
fn runtime_loop_collector_preserves_phase_order_for_maintenance_events_and_calendar_timers() {
    let before_id = operation_id(1);
    let after_id = operation_id(2);
    let calendar_id = operation_id(3);
    let maintenance_before_wait = SupervisorOperationMaintenanceTurn {
        operation_timeouts: vec![completed_operation(before_id, "before_wait")],
        ..SupervisorOperationMaintenanceTurn::default()
    };
    let shutdown_turn = RuntimeShutdownEventTurn::ShutdownDeadlineTimer {
        read: LinuxTimerFdRead::Expired { expirations: 1 },
        drive: Some(Box::new(SupervisorShutdownDriveDispatch {
            timeout: Some(SupervisorShutdownTimeoutDispatch {
                submitted: Vec::new(),
                global_timeout: false,
                cgroup_kills: Vec::new(),
                job_events: Vec::new(),
                abandoned: vec![abandoned_dispatch("abandoned")],
                next_wave: Vec::new(),
                finalization: ShutdownFinalizationState::WaitingForServices,
            }),
            finalization: None,
        })),
        deadline_timer: SupervisorShutdownDeadlineTimerTurn::Disarmed,
    };
    let maintenance_after_sources = SupervisorOperationMaintenanceTurn {
        operation_timeouts: vec![completed_operation(after_id, "after_sources")],
        relationship_audit_events: vec![SupervisorOnFailureLoopSuppressedDispatch {
            failed_service: "a".to_string(),
            attempted_handler: "b".to_string(),
            chain: vec!["b".to_string(), "a".to_string(), "b".to_string()],
            reason: SupervisorOnFailureLoopSuppressionReason::Cycle,
        }],
        ..SupervisorOperationMaintenanceTurn::default()
    };
    let calendar_turn = RuntimeCalendarTimerTurn::Read {
        read: LinuxTimerFdRead::Expired { expirations: 1 },
        supervisor: Some(Box::new(timer_start_dispatch(calendar_id, "calendar"))),
        last_run_write: None,
        next_scheduled_ns: None,
    };

    let mut events = Vec::<KmesEvent>::new();
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &maintenance_before_wait,
        &[shutdown_turn],
        &RuntimeWorkPumpTurn::default(),
        &maintenance_after_sources,
        &[(17, calendar_turn)],
        &mut events,
    )
    .expect("collected KMES events");

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec![
            "operation.completed",
            "shutdown.abandoned",
            "operation.completed",
            "on_failure.loop_suppressed",
            "operation.requested",
        ],
    );
}

#[test]
fn runtime_loop_collector_emits_access_denial_audit_events() {
    let command_error =
        SupervisorControlCommandBodyError::ServiceAccessDenied(Box::new(ServiceAccessDenied {
            caller: TokenSummary::requested_identity("S-1-5-21-client"),
            service: "secret".to_string(),
            desired_access: ServiceAccess::QUERY_STATUS,
            granted_access_bits: 0,
        }));
    let control_turn = RuntimeShutdownEventTurn::ControlConnection {
        fd: 44,
        supervisor: Box::new(SupervisorControlConnectionTableTurn {
            fd: 44,
            turn: SupervisorControlConnectionTurn {
                read: ControlConnectionReadTurn::WouldBlock { buffered_bytes: 0 },
                frames: vec![SupervisorControlConnectionFrameTurn {
                    frame: SupervisorControlFrameTurn::CommandRejected {
                        response_line: b"{\"status\":\"error\",\"code\":\"ACCESS_DENIED\"}\n"
                            .to_vec(),
                        error: command_error,
                        remaining_bytes: 0,
                    },
                    pending_write_bytes: 0,
                    close_after_write: false,
                }],
                write: ControlConnectionWriteTurn::Idle {
                    close_after_write: false,
                },
                close_connection: false,
            },
            removed: false,
            active_connections: 1,
        }),
        deadline_timer: None,
    };

    let mut events = Vec::<KmesEvent>::new();
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[control_turn],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
        &mut events,
    )
    .expect("collected KMES events");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "access.denied");
}

fn completed_operation(id: crate::ids::OperationId, service: &str) -> OperationEvent {
    OperationEvent::completed(&OperationRecord {
        id,
        operation_type: OperationType::Start,
        service: service.to_string(),
        state: OperationState::Completed,
        created_at_ns: 10,
        lifetime_from_ns: 10,
        started_at_ns: Some(12),
        completed_at_ns: Some(20),
        source: OperationSource::Admin,
        caller: None,
        result: Some("timed out".to_string()),
        merged_into: None,
        service_security: None,
    })
    .expect("completed operation")
}

fn requested_operation(id: crate::ids::OperationId, service: &str) -> OperationEvent {
    OperationEvent::requested(&OperationRecord {
        id,
        operation_type: OperationType::Start,
        service: service.to_string(),
        state: OperationState::Pending,
        created_at_ns: 30,
        lifetime_from_ns: 30,
        started_at_ns: None,
        completed_at_ns: None,
        source: OperationSource::Timer,
        caller: None,
        result: None,
        merged_into: None,
        service_security: None,
    })
}

fn timer_start_dispatch(
    operation_id: crate::ids::OperationId,
    service: &str,
) -> SupervisorTimerDispatch {
    SupervisorTimerDispatch {
        service: service.to_string(),
        schedule: "daily".to_string(),
        action: SupervisorTimerAction::Start {
            requested_operation_id: operation_id,
            outcome: Box::new(OnDemandStartDispatch {
                plan: OnDemandStartPlan {
                    requested: service.to_string(),
                    requested_operation_source: OperationSource::Timer,
                    requested_transition_cause: TransitionCause::Timer,
                    starts: Vec::new(),
                    blocked: Vec::new(),
                },
                requested_operation: OperationRequestOutcome {
                    returned_operation_id: operation_id,
                    stored_operation_id: operation_id,
                    decision: OperationConflictDecision::CreateNew,
                    events: vec![requested_operation(operation_id, service)],
                },
                dependency_operations: Vec::new(),
                events: vec![requested_operation(operation_id, service)],
            }),
            context_id: GraphContextId::new_for_test(9),
            start_dispatches: Vec::new(),
        },
    }
}

fn abandoned_dispatch(service: &str) -> SupervisorShutdownAbandonedDispatch {
    SupervisorShutdownAbandonedDispatch {
        service: service.to_string(),
        cgroup_id: format!("system.slice/{service}"),
        service_transition: ServiceTableTransition {
            event: ServiceTransitionEvent {
                service: service.to_string(),
                from: ServiceState::Stopping,
                to: ServiceState::Abandoned,
                cause: TransitionCause::ProcessUnkillable,
                generation: 4,
            },
            discarded_definition_removed: false,
            released_tty: None,
        },
    }
}

fn operation_id(sequence: u64) -> crate::ids::OperationId {
    OperationIdAllocator::with_next_sequence(sequence)
        .allocate_batch(1, 1_717_171_717_123_456_789)
        .expect("operation id")[0]
}

/// PEI-1125: a contained internal error leaves a complete trail — the job it
/// retired, the operation it failed, and a `service.internal_error` saying
/// what peinit could not do — so the failure does not read as the service's.
#[test]
fn runtime_loop_collector_records_a_contained_internal_error() {
    let dispatch = crate::supervisor::SupervisorInternalErrorDispatch {
        step: "lifecycle deadline",
        subject: crate::supervisor::SupervisorInternalErrorSubject {
            service: Some("app".to_string()),
            job_id: None,
        },
        error: "InvalidTransition".to_string(),
        observed_at_ns: 40,
        job_event: None,
        service_job_event: None,
        operation_event: Some(completed_operation(operation_id(7), "app")),
        service_transition: Some(ServiceTableTransition {
            event: ServiceTransitionEvent {
                service: "app".to_string(),
                from: ServiceState::Starting,
                to: ServiceState::Failed,
                cause: TransitionCause::InternalError,
                generation: 2,
            },
            discarded_definition_removed: false,
            released_tty: None,
        }),
        start_dispatches: Vec::new(),
    };
    let turn = RuntimeShutdownEventTurn::LifecycleDeadlineTimer {
        read: LinuxTimerFdRead::Expired { expirations: 1 },
        drive: Some(Box::new(
            crate::supervisor::SupervisorLifecycleDeadlineDispatch {
                internal_errors: vec![dispatch],
                ..crate::supervisor::SupervisorLifecycleDeadlineDispatch::default()
            },
        )),
        deadline_timer: crate::supervisor::SupervisorLifecycleDeadlineTimerTurn::Disarmed,
    };

    let mut events = Vec::new();
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
        &mut events,
    )
    .expect("collected KMES events");

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["operation.completed", "service.internal_error"],
    );
    assert_eq!(
        crate::kmes::kmes_event_subject(&events[1].payload)
            .0
            .as_deref(),
        Some("app")
    );
}

/// PEI-1082: a `job.ended` whose arguments were cut is followed by an
/// `event.oversized` saying so, so the audit trail records the gap.
#[test]
fn a_cut_job_ended_is_followed_by_an_event_oversized_naming_the_cut() {
    let job_id = crate::ids::JobIdAllocator::new()
        .allocate_batch(1, 1_717_171_717_123_456_789)
        .expect("job id")[0];
    let job_event = crate::job::JobEvent::ended(&crate::job::JobRecord {
        id: job_id,
        service: Some("app".to_string()),
        job_type: crate::job::JobType::ServiceMain,
        hook_index: None,
        state: crate::job::JobState::Failed,
        pid: Some(800),
        pidfd: Some(80),
        resolved_identity: "SYSTEM".to_string(),
        token_summary: TokenSummary::requested_identity("SYSTEM"),
        required_privileges: Vec::new(),
        image_path: "/sbin/app".to_string(),
        arguments: (0..20).map(|_| "a".repeat(4096)).collect(),
        environment: Vec::new(),
        working_directory: "/".to_string(),
        limit_nofile: None,
        limit_core: None,
        oom_score_adj: 0,
        created_at_ns: 10,
        started_at_ns: Some(20),
        ended_at_ns: Some(30),
        exit_code: None,
        exit_signal: None,
        failure_cause: Some("internal_error: job terminal: x".to_string()),
        cgroup_id: "system.slice/app".to_string(),
        activation_generation: 1,
        cgroup_generation: 0,
        operation_id: None,
        console_path: None,
    })
    .expect("job event");
    let turn = RuntimeShutdownEventTurn::DeferredChildReaps {
        child_reaps: vec![crate::supervisor::SupervisorChildReapTurn::InternalError {
            child: crate::boundary::ChildReap {
                pid: 800,
                status: crate::boundary::ChildExitStatus::Exited { code: 0 },
            },
            dispatch: Box::new(crate::supervisor::SupervisorInternalErrorDispatch {
                step: "job terminal",
                subject: crate::supervisor::SupervisorInternalErrorSubject {
                    service: Some("app".to_string()),
                    job_id: Some(job_id),
                },
                error: "x".to_string(),
                observed_at_ns: 30,
                job_event: Some(job_event),
                service_job_event: None,
                operation_event: None,
                service_transition: None,
                start_dispatches: Vec::new(),
            }),
        }],
        ended_at_ns: 30,
    };

    let mut events = Vec::new();
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
        &mut events,
    )
    .expect("collected KMES events");

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["job.ended", "event.oversized", "service.internal_error"],
    );
    assert!(events[0].payload.len() < 65_536);
    assert_eq!(
        crate::kmes::kmes_event_subject(&events[1].payload),
        (Some("app".to_string()), Some(job_id.to_string())),
    );
    assert_eq!(read_str(&events[1].payload, "action"), "truncated");
    assert_eq!(read_str(&events[1].payload, "event"), "job.ended");
}

fn read_str(payload: &[u8], field: &str) -> String {
    let mut reader = peios::msgpack::Reader::new(payload);
    let count = reader.read_map().expect("payload map");
    for _ in 0..count {
        let key = reader.read_str().expect("field key");
        if key == field {
            return reader.read_str().expect("field value").to_string();
        }
        reader.skip().expect("skip value");
    }
    panic!("missing field {field}");
}
