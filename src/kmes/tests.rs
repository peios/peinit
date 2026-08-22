use peios::msgpack::Reader;

use crate::boot::phase2::{BlockedReason, Phase2BootPlanError, Phase2BootRunError};
use crate::control::service_security::{ServiceAccess, ServiceAccessDenied};
use crate::control::system::{SystemAccess, SystemAccessDenied};
use crate::execution::notify::{AuthenticatedNotifySender, NotifyAppliedField};
use crate::fd_store::StoreFdOutcome;
use crate::ids::{JobId, JobIdAllocator, OperationId, OperationIdAllocator};
use crate::init::InitRecoveryReason;
use crate::job::{JobEvent, JobRecord, JobState, JobType};
use crate::operation::store::OperationEvent;
use crate::operation::{OperationRecord, OperationSource, OperationState, OperationType};
use crate::security::TokenSummary;
use crate::service::ServiceTableTransition;
use crate::service::runtime::{ServiceState, ServiceTransitionEvent, TransitionCause};
use crate::service::{ServiceDependencyKind, ServiceGraphFinding, ServiceGraphWarning};
use crate::shutdown::{CleanupActionResult, ShutdownFinalizationReport, ShutdownFinalizationState};
use crate::supervisor::{
    SupervisorError, SupervisorFdStoreRejectionDispatch, SupervisorOnFailureLoopSuppressedDispatch,
    SupervisorOnFailureLoopSuppressionReason, SupervisorShutdownAbandonedDispatch,
    SupervisorShutdownFinalizationDispatch,
};

use super::{
    encode_boot_blocked_service_event, encode_critical_failure_event,
    encode_fd_store_rejection_event, encode_graph_validation_error_event,
    encode_graph_validation_warning_event, encode_init_recovery_events, encode_job_event,
    encode_notify_applied_field_events, encode_notify_rejection_event,
    encode_on_failure_loop_suppressed_event, encode_operation_event,
    encode_service_access_denied_event, encode_shutdown_abandoned_event,
    encode_system_access_denied_event,
};

const OBSERVED_AT_NS: u64 = 1_717_171_717_123_456_789;

#[test]
fn encodes_terminal_job_payload_as_msgpack_record() {
    let operation_id = operation_id();
    let job_id = job_id();
    let event = JobEvent::ended(&JobRecord {
        id: job_id,
        service: Some("app".to_string()),
        job_type: JobType::ServiceMain,
        hook_index: None,
        state: JobState::Failed,
        pid: Some(123),
        pidfd: Some(44),
        resolved_identity: "SYSTEM".to_string(),
        token_summary: token("SYSTEM"),
        required_privileges: Vec::new(),
        image_path: "/sbin/app".to_string(),
        arguments: vec!["--foreground".to_string(), "--ready".to_string()],
        environment: Vec::new(),
        working_directory: "/".to_string(),
        limit_nofile: None,
        limit_core: None,
        oom_score_adj: 0,
        created_at_ns: 1_000,
        started_at_ns: Some(1_100),
        ended_at_ns: Some(1_500),
        exit_code: Some(2),
        exit_signal: None,
        failure_cause: Some("exit-code".to_string()),
        cgroup_id: "system.slice/app".to_string(),
        activation_generation: 3,
        cgroup_generation: 7,
        operation_id: Some(operation_id),
        console_path: None,
    })
    .expect("job ended event");

    let encoded = encode_job_event(&event).expect("encoded event");

    assert_eq!(encoded.event_type, "job.ended");
    assert_eq!(read_str(&encoded.payload, "job_id"), job_id.to_string());
    assert_eq!(read_str(&encoded.payload, "service"), "app");
    assert_eq!(read_str(&encoded.payload, "type"), "service_main");
    assert_eq!(read_str(&encoded.payload, "final_state"), "failed");
    assert_eq!(
        read_str(&encoded.payload, "operation_id"),
        operation_id.to_string()
    );
    assert_eq!(read_uint(&encoded.payload, "pid"), 123);
    assert_eq!(read_int(&encoded.payload, "pidfd"), 44);
    assert_eq!(read_uint(&encoded.payload, "duration_ns"), 500);
    assert_eq!(read_int(&encoded.payload, "exit_code"), 2);
    assert_nil(&encoded.payload, "exit_signal");
    assert_eq!(
        read_str_array(&encoded.payload, "arguments"),
        ["--foreground", "--ready"]
    );
}

#[test]
fn encodes_operation_terminal_payload() {
    let operation_id = operation_id();
    let event = OperationEvent::completed(&OperationRecord {
        id: operation_id,
        operation_type: OperationType::Start,
        service: "app".to_string(),
        state: OperationState::Completed,
        created_at_ns: 10,
        started_at_ns: Some(12),
        completed_at_ns: Some(45),
        source: OperationSource::Boot,
        caller: Some(token("SYSTEM")),
        result: Some("started".to_string()),
        merged_into: None,
    })
    .expect("operation event");

    let encoded = encode_operation_event(&event).expect("encoded event");

    assert_eq!(encoded.event_type, "operation.completed");
    assert_eq!(
        read_str(&encoded.payload, "operation_id"),
        operation_id.to_string()
    );
    assert_eq!(read_str(&encoded.payload, "type"), "start");
    assert_eq!(read_str(&encoded.payload, "source"), "boot");
    assert_eq!(read_str(&encoded.payload, "state"), "completed");
    assert_eq!(read_uint(&encoded.payload, "duration_ns"), 35);
    assert_eq!(read_str(&encoded.payload, "result"), "started");
    assert_eq!(
        read_nested_str(&encoded.payload, "caller", "identity"),
        "SYSTEM"
    );
}

#[test]
fn encodes_only_event_emitting_notify_fields() {
    let sender = AuthenticatedNotifySender {
        service: "app".to_string(),
        job_id: job_id(),
        operation_id: Some(operation_id()),
        generation: 3,
        job_created_at_ns: 1_000,
        cgroup_generation: 7,
    };

    let events = encode_notify_applied_field_events(
        &sender,
        &[
            NotifyAppliedField::Ready,
            NotifyAppliedField::Status {
                text: "warming cache".to_string(),
            },
            NotifyAppliedField::Errno {
                value: "5".to_string(),
            },
            NotifyAppliedField::ExitStatus {
                value: "75".to_string(),
            },
            NotifyAppliedField::Watchdog,
        ],
    )
    .expect("notify events");

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["notify.status", "notify.errno", "notify.exit_status"]
    );
    assert_eq!(read_str(&events[0].payload, "service"), "app");
    assert_eq!(
        read_str(&events[0].payload, "job_id"),
        sender.job_id.to_string()
    );
    assert_eq!(read_uint(&events[0].payload, "generation"), 3);
    assert_eq!(read_str(&events[0].payload, "status"), "warming cache");
    assert_eq!(read_str(&events[1].payload, "errno"), "5");
    assert_eq!(read_str(&events[2].payload, "exit_status"), "75");
}

#[test]
fn encodes_fd_store_rejection_payload() {
    let event = encode_fd_store_rejection_event(&SupervisorFdStoreRejectionDispatch {
        service: "app".to_string(),
        name: "api".to_string(),
        outcome: StoreFdOutcome::Full,
    })
    .expect("fd-store rejection event");

    assert_eq!(event.event_type, "fd_store.rejected");
    assert_eq!(read_str(&event.payload, "service"), "app");
    assert_eq!(read_str(&event.payload, "name"), "api");
    assert_eq!(read_str(&event.payload, "outcome"), "full");
    assert_eq!(read_str(&event.payload, "reason"), "full");
}

#[test]
fn encodes_notify_rejection_with_optional_attribution() {
    let sender = AuthenticatedNotifySender {
        service: "app".to_string(),
        job_id: job_id(),
        operation_id: None,
        generation: 8,
        job_created_at_ns: 1_000,
        cgroup_generation: 7,
    };

    let event = encode_notify_rejection_event(Some(55), "parse error", Some(&sender))
        .expect("notify rejection event");

    assert_eq!(event.event_type, "notify.rejected");
    assert_eq!(read_uint(&event.payload, "sender_pid"), 55);
    assert_eq!(read_str(&event.payload, "reason"), "parse error");
    assert_eq!(read_str(&event.payload, "service"), "app");
    assert_eq!(
        read_str(&event.payload, "job_id"),
        sender.job_id.to_string()
    );
    assert_nil(&event.payload, "operation_id");
    assert_eq!(read_uint(&event.payload, "generation"), 8);
}

#[test]
fn encodes_graph_validation_audit_payloads() {
    let warning = encode_graph_validation_warning_event(
        "reload_config",
        &ServiceGraphWarning::AliveReadinessWithHardDependents {
            service: "db".to_string(),
            dependents: vec!["app".to_string()],
        },
    )
    .expect("graph warning event");
    let error = encode_graph_validation_error_event(
        "reload_config",
        &ServiceGraphFinding::MissingHardDependency {
            service: "app".to_string(),
            target: "db".to_string(),
            kind: ServiceDependencyKind::Requires,
        },
    )
    .expect("graph error event");

    assert_eq!(warning.event_type, "graph.validation_warning");
    assert_eq!(read_str(&warning.payload, "phase"), "reload_config");
    assert_eq!(
        read_str(&warning.payload, "warning"),
        "alive_readiness_with_hard_dependents"
    );
    assert_eq!(read_str_array(&warning.payload, "dependents"), ["app"]);
    assert_eq!(error.event_type, "graph.validation_error");
    assert_eq!(
        read_str(&error.payload, "finding"),
        "missing_hard_dependency"
    );
    assert_eq!(read_str(&error.payload, "target"), "db");
    assert_eq!(read_str(&error.payload, "dependency_kind"), "requires");

    let timer_error = encode_graph_validation_error_event(
        "reload_config",
        &ServiceGraphFinding::InvalidTimerSchedule {
            service: "timer".to_string(),
            schedule: "*-*-* 12:00:00.5 UTC".to_string(),
            message: "fractional seconds are not supported".to_string(),
        },
    )
    .expect("timer graph error event");
    assert_eq!(
        read_str(&timer_error.payload, "finding"),
        "invalid_timer_schedule",
    );
    assert_eq!(
        read_str(&timer_error.payload, "schedule"),
        "*-*-* 12:00:00.5 UTC",
    );
    assert_eq!(
        read_str(&timer_error.payload, "parse_error"),
        "fractional seconds are not supported",
    );
}

#[test]
fn encodes_access_denial_payloads_with_requested_rights() {
    let system = encode_system_access_denied_event(&SystemAccessDenied {
        caller: token("S-1-5-21-client"),
        desired_access: SystemAccess::SHUTDOWN,
        granted_access_bits: 0,
    })
    .expect("system access denial");
    let service = encode_service_access_denied_event(&ServiceAccessDenied {
        caller: token("S-1-5-21-client"),
        service: "app".to_string(),
        desired_access: ServiceAccess::START.union(ServiceAccess::STOP),
        granted_access_bits: ServiceAccess::START.bits(),
    })
    .expect("service access denial");

    assert_eq!(system.event_type, "access.denied");
    assert_eq!(read_str(&system.payload, "caller_sid"), "S-1-5-21-client");
    assert_eq!(read_str(&system.payload, "target_type"), "system");
    assert_eq!(read_str(&system.payload, "target"), "peinit_control");
    assert_eq!(
        read_str(&system.payload, "requested_right"),
        "SYSTEM_SHUTDOWN"
    );
    assert_eq!(service.event_type, "access.denied");
    assert_eq!(read_str(&service.payload, "target_type"), "service");
    assert_eq!(read_str(&service.payload, "target"), "app");
    assert_eq!(
        read_str(&service.payload, "requested_right"),
        "SERVICE_START|SERVICE_STOP"
    );
    assert_eq!(read_uint(&service.payload, "granted_access_bits"), 2);
}

#[test]
fn encodes_on_failure_loop_suppression_payload() {
    let event =
        encode_on_failure_loop_suppressed_event(&SupervisorOnFailureLoopSuppressedDispatch {
            failed_service: "a".to_string(),
            attempted_handler: "b".to_string(),
            chain: vec!["b".to_string(), "a".to_string(), "b".to_string()],
            reason: SupervisorOnFailureLoopSuppressionReason::Cycle,
        })
        .expect("on-failure loop suppression event");

    assert_eq!(event.event_type, "on_failure.loop_suppressed");
    assert_eq!(read_str(&event.payload, "failed_service"), "a");
    assert_eq!(read_str(&event.payload, "attempted_handler"), "b");
    assert_eq!(read_str_array(&event.payload, "chain"), ["b", "a", "b"]);
    assert_eq!(read_str(&event.payload, "reason"), "cycle");
}

#[test]
fn encodes_recovery_entry_and_phase2_graph_error() {
    let events = encode_init_recovery_events(&InitRecoveryReason::Phase2(
        SupervisorError::Phase2Boot(Phase2BootRunError::Plan(Phase2BootPlanError::Cycle {
            services: vec!["a".to_string(), "b".to_string(), "a".to_string()],
        })),
    ))
    .expect("recovery events");

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["recovery.entered", "graph.validation_error"],
    );
    assert_eq!(read_str(&events[0].payload, "reason"), "phase2");
    assert_eq!(read_str(&events[1].payload, "phase"), "phase2_boot");
    assert_eq!(read_str(&events[1].payload, "finding"), "cycle");
    assert_eq!(
        read_str_array(&events[1].payload, "services"),
        ["a", "b", "a"]
    );
}

#[test]
fn encodes_shutdown_abandonment_and_critical_failure() {
    let abandoned = encode_shutdown_abandoned_event(&SupervisorShutdownAbandonedDispatch {
        service: "app".to_string(),
        cgroup_id: "system.slice/app".to_string(),
        service_transition: ServiceTableTransition {
            event: ServiceTransitionEvent {
                service: "app".to_string(),
                from: ServiceState::Stopping,
                to: ServiceState::Abandoned,
                cause: TransitionCause::ProcessUnkillable,
                generation: 6,
            },
            discarded_definition_removed: false,
        },
    })
    .expect("shutdown abandoned event");
    let critical = encode_critical_failure_event(
        "app",
        "watchdog_timeout",
        Some(42),
        &SupervisorShutdownFinalizationDispatch {
            report: empty_finalization_report(),
            finalization: ShutdownFinalizationState::Completed,
        },
    )
    .expect("critical failure event");

    assert_eq!(abandoned.event_type, "shutdown.abandoned");
    assert_eq!(read_str(&abandoned.payload, "service"), "app");
    assert_eq!(read_str(&abandoned.payload, "to_state"), "abandoned");
    assert_eq!(read_str(&abandoned.payload, "cause"), "process_unkillable");
    assert_eq!(critical.event_type, "critical.failure");
    assert_eq!(read_str(&critical.payload, "service"), "app");
    assert_eq!(read_str(&critical.payload, "trigger"), "watchdog_timeout");
    assert_eq!(read_uint(&critical.payload, "observed_at_ns"), 42);
    assert_eq!(read_str(&critical.payload, "final_action"), "reboot");
}

fn read_str(payload: &[u8], field: &str) -> String {
    read_field(payload, field, |reader| {
        reader.read_str().map(str::to_string)
    })
}

fn read_nested_str(payload: &[u8], field: &str, nested: &str) -> String {
    read_field(payload, field, |reader| {
        let count = reader.read_map()?;
        for _ in 0..count {
            let key = reader.read_str()?;
            if key == nested {
                return reader.read_str().map(str::to_string);
            }
            reader.skip()?;
        }
        panic!("missing nested field {nested}");
    })
}

fn read_uint(payload: &[u8], field: &str) -> u64 {
    read_field(payload, field, |reader| reader.read_uint())
}

fn read_int(payload: &[u8], field: &str) -> i64 {
    read_field(payload, field, |reader| reader.read_int())
}

fn read_str_array(payload: &[u8], field: &str) -> Vec<String> {
    read_field(payload, field, |reader| {
        let count = reader.read_array()?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(reader.read_str()?.to_string());
        }
        Ok(values)
    })
}

fn assert_nil(payload: &[u8], field: &str) {
    read_field(payload, field, |reader| reader.read_nil())
}

fn read_field<T, F>(payload: &[u8], field: &str, mut read: F) -> T
where
    F: FnMut(&mut Reader<'_>) -> peios::Result<T>,
{
    let mut reader = Reader::new(payload);
    let count = reader.read_map().expect("payload map");
    for _ in 0..count {
        let key = reader.read_str().expect("field key");
        if key == field {
            return read(&mut reader).expect("field value");
        }
        reader.skip().expect("skip value");
    }
    panic!("missing field {field}");
}

fn job_id() -> JobId {
    JobIdAllocator::new()
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect("job id")[0]
}

fn operation_id() -> OperationId {
    OperationIdAllocator::new()
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect("operation id")[0]
}

fn token(identity: &str) -> TokenSummary {
    TokenSummary::requested_identity(identity)
}

fn empty_finalization_report() -> ShutdownFinalizationReport {
    ShutdownFinalizationReport {
        random_seed: CleanupActionResult::Ok,
        snapshot_mounts: CleanupActionResult::Ok,
        mount_results: Vec::new(),
        root_remount: CleanupActionResult::Ok,
        sync_result: CleanupActionResult::Ok,
        reboot_result: CleanupActionResult::Ok,
    }
}

/// Boot findings reuse `graph.validation_error` and are separated from reload
/// findings by `phase`, so one consumer filter catches both regimes.
#[test]
fn encodes_boot_blocked_service_payloads() {
    let missing = encode_boot_blocked_service_event(
        "app",
        &BlockedReason::HardDependencyUnavailable {
            target: "db".to_string(),
            kind: ServiceDependencyKind::Requires,
        },
    )
    .expect("missing dependency event");

    assert_eq!(missing.event_type, "graph.validation_error");
    assert_eq!(read_str(&missing.payload, "phase"), "boot");
    assert_eq!(
        read_str(&missing.payload, "finding"),
        "missing_hard_dependency"
    );
    assert_eq!(read_str(&missing.payload, "service"), "app");
    assert_eq!(read_str(&missing.payload, "target"), "db");
    assert_eq!(read_str(&missing.payload, "dependency_kind"), "requires");

    // The finding reload cannot produce: blocked because a dependency is
    // blocked, which is not the same claim as the dependency being missing.
    let blocked = encode_boot_blocked_service_event(
        "app",
        &BlockedReason::HardDependencyBlocked {
            target: "db".to_string(),
            kind: ServiceDependencyKind::BindsTo,
        },
    )
    .expect("blocked dependency event");
    assert_eq!(
        read_str(&blocked.payload, "finding"),
        "hard_dependency_blocked"
    );
    assert_eq!(read_str(&blocked.payload, "dependency_kind"), "binds_to");

    let cycle = encode_boot_blocked_service_event(
        "a",
        &BlockedReason::CycleDetected {
            services: vec!["a".to_string(), "b".to_string(), "a".to_string()],
        },
    )
    .expect("cycle event");
    assert_eq!(read_str(&cycle.payload, "finding"), "cycle");
    assert_eq!(read_str_array(&cycle.payload, "services"), ["a", "b", "a"]);

    let conflict = encode_boot_blocked_service_event(
        "a",
        &BlockedReason::ConflictingBootService {
            target: "b".to_string(),
        },
    )
    .expect("conflict event");
    assert_eq!(
        read_str(&conflict.payload, "finding"),
        "conflicting_boot_services"
    );
    assert_eq!(read_str(&conflict.payload, "target"), "b");

    let validation = encode_boot_blocked_service_event(
        "app",
        &BlockedReason::ValidationError {
            message: "health check interval exceeds the restart window".to_string(),
        },
    )
    .expect("validation event");
    assert_eq!(read_str(&validation.payload, "finding"), "validation_error");
    assert_eq!(
        read_str(&validation.payload, "detail"),
        "health check interval exceeds the restart window"
    );
}
