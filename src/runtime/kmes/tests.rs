use crate::boundary::LinuxTimerFdRead;
use crate::kmes::EventCollector;
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

    let mut out = EventCollector::everything(Default::default());
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
        &mut out,
    )
    .expect("collected KMES events");
    let events = out.into_events();

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["peinit.cgroup.leaked"],
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
                waiting_for: Vec::new(),
                still_waiting_for: Vec::new(),
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

    let mut out = EventCollector::everything(Default::default());
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &maintenance_before_wait,
        &[shutdown_turn],
        &RuntimeWorkPumpTurn::default(),
        &maintenance_after_sources,
        &[(17, calendar_turn)],
        &mut out,
    )
    .expect("collected KMES events");
    let events = out.into_events();

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec![
            "peinit.operation.ended",
            "peinit.service.abandoned",
            "peinit.operation.ended",
            "peinit.on-failure.suppressed",
            "peinit.operation.requested",
        ],
    );
}

/// The emission policy decides per type, before the payload is built: with
/// `peinit.operation.ended` switched off, the turn's other events are
/// written in their order and that type is not.
#[test]
fn runtime_loop_collector_leaves_out_a_type_the_policy_switches_off() {
    let maintenance = SupervisorOperationMaintenanceTurn {
        operation_timeouts: vec![completed_operation(operation_id(1), "a")],
        relationship_audit_events: vec![SupervisorOnFailureLoopSuppressedDispatch {
            failed_service: "a".to_string(),
            attempted_handler: "b".to_string(),
            chain: vec!["b".to_string()],
            reason: SupervisorOnFailureLoopSuppressionReason::Cycle,
        }],
        ..SupervisorOperationMaintenanceTurn::default()
    };
    let policy = |event_type: &str, _: crate::boundary::EventTier| {
        event_type != "peinit.operation.ended"
    };
    let mut out = EventCollector::new(&policy, Default::default());
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &maintenance,
        &[],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
        &mut out,
    )
    .expect("collected KMES events");

    assert_eq!(
        out.events()
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["peinit.on-failure.suppressed"],
    );
}

/// PEI-1279: a command refused by a descriptor writes no event of peinit's.
/// KACS records the decision, as `kacs.audit.access.checked`, under the
/// descriptor's SACL.
#[test]
fn runtime_loop_collector_writes_no_event_for_an_access_denial() {
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

    let mut out = EventCollector::everything(Default::default());
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[control_turn],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
        &mut out,
    )
    .expect("collected KMES events");
    let events = out.into_events();

    assert!(events.is_empty(), "{events:?}");
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
/// retired, the operation it failed, and a `peinit.internal-error.contained`
/// saying what peinit could not do — so the failure does not read as the
/// service's.
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

    let mut out = EventCollector::everything(Default::default());
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
        &mut out,
    )
    .expect("collected KMES events");
    let events = out.into_events();

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["peinit.operation.ended", "peinit.internal-error.contained"],
    );
    assert_eq!(
        crate::kmes::kmes_event_subject(&events[1].payload)
            .service
            .as_deref(),
        Some("app")
    );
}

/// PEI-1082: a `peinit.job.ended` whose arguments were cut says so itself;
/// the `truncated` notice that used to follow it is withdrawn.
#[test]
fn a_cut_job_ended_says_it_was_cut_and_needs_no_notice() {
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

    let mut out = EventCollector::everything(Default::default());
    collect_runtime_loop_kmes_events(
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[turn],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
        &mut out,
    )
    .expect("collected KMES events");
    let events = out.into_events();

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["peinit.job.ended", "peinit.internal-error.contained"],
    );
    assert!(events[0].payload.len() < 65_536);
    assert_eq!(
        crate::kmes::kmes_event_subject(&events[0].payload),
        crate::kmes::KmesEventSubject {
            service: Some("app".to_string()),
            job_guid: Some(job_id.as_guid_bytes()),
        },
    );
    assert!(read_bool(&events[0].payload, &["object", "job", "arguments-truncated"]));
}

fn read_bool(payload: &[u8], path: &[&str]) -> bool {
    let mut reader = peios::msgpack::Reader::new(payload);
    'segments: for segment in path {
        let count = reader.read_map().expect("payload map");
        for _ in 0..count {
            let key = reader.read_str().expect("field key");
            if key == *segment {
                continue 'segments;
            }
            reader.skip().expect("skip value");
        }
        panic!("missing field {segment}");
    }
    reader.read_bool().expect("field value")
}
