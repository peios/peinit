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
    encode_registry_reload_coalesced_event, encode_registry_reload_deferred_event,
    encode_reload_undecodable_service_event, encode_service_access_denied_event,
    encode_shutdown_abandoned_event, encode_system_access_denied_event,
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
        lifetime_from_ns: 10,
        started_at_ns: Some(12),
        completed_at_ns: Some(45),
        source: OperationSource::Boot,
        caller: Some(token("SYSTEM")),
        result: Some("started".to_string()),
        merged_into: None,
        service_security: None,
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

/// PEI-350: the boot window's deferral and the coalesced reload that follows
/// it are both on the audit trail, so a consumer can tell when the boot's
/// snapshot was let go of.
#[test]
fn encodes_the_boot_window_deferral_and_the_coalesced_reload() {
    let deferred = encode_registry_reload_deferred_event(&["app".to_string(), "db".to_string()])
        .expect("deferred event");
    assert_eq!(deferred.event_type, "config.reload_deferred");
    assert_eq!(read_uint(&deferred.payload, "events"), 2);
    assert_eq!(read_str_array(&deferred.payload, "services"), ["app", "db"]);

    let outcome = crate::control::reload_config::ReloadConfigOutcome {
        summary: crate::service::ServiceReloadSummary {
            added: vec!["new".to_string()],
            updated: Vec::new(),
            restored: Vec::new(),
            marked_removed: Vec::new(),
            discarded: Vec::new(),
            undecodable: vec!["broken".to_string()],
            deferred: Vec::new(),
        },
        services_schema_version: 1,
        config_warnings: Vec::new(),
        control_security: crate::control::system::ControlSecurityDescriptor::Default,
        control_limits: crate::control::socket::ControlSocketLimits::default(),
        jobs_limits: crate::jobs::socket::JobsSocketLimits::default(),
        log_config: crate::logging::RuntimeLogConfig::default(),
        shutdown_settings: crate::shutdown::ShutdownSettings::default(),
        global_environment: Vec::new(),
        eventd_log_socket_path: None,
        warnings: Vec::new(),
        undecodable: Vec::new(),
    };
    let coalesced = encode_registry_reload_coalesced_event(
        &crate::supervisor::DeferredRegistryReload {
            services: vec!["app".to_string(), "db".to_string()],
        },
        &Ok(outcome),
    )
    .expect("coalesced event");
    assert_eq!(coalesced.event_type, "config.reload_coalesced");
    assert_eq!(read_uint(&coalesced.payload, "deferred"), 2);
    assert_eq!(
        read_str_array(&coalesced.payload, "services"),
        ["app", "db"]
    );
    assert_eq!(read_str(&coalesced.payload, "result"), "ok");
    assert_eq!(read_uint(&coalesced.payload, "added"), 1);
    assert_eq!(read_uint(&coalesced.payload, "undecodable"), 1);

    let failed = encode_registry_reload_coalesced_event(
        &crate::supervisor::DeferredRegistryReload::default(),
        &Err(crate::control::reload_config::ReloadConfigError::Registry(
            crate::boundary::BoundaryError::Registry("offline".to_string()),
        )),
    )
    .expect("failed coalesced event");
    assert_eq!(read_str(&failed.payload, "result"), "error");
    assert_eq!(
        read_str(&failed.payload, "error"),
        "Registry(Registry(\"offline\"))"
    );
}

/// PEI-621: a key a reload could not decode is audited as the same
/// `validation_error` finding the boot emits for one, under the
/// `reload_config` phase.
#[test]
fn encodes_reload_undecodable_service_as_a_validation_error() {
    let event = encode_reload_undecodable_service_event(&crate::boundary::UndecodableService {
        name: "broken".to_string(),
        field: Some("ImagePath".to_string()),
        message: "MalformedString { field: \"ImagePath\" }".to_string(),
    })
    .expect("undecodable event");

    assert_eq!(event.event_type, "graph.validation_error");
    assert_eq!(read_str(&event.payload, "phase"), "reload_config");
    assert_eq!(read_str(&event.payload, "finding"), "validation_error");
    assert_eq!(read_str(&event.payload, "service"), "broken");
    assert_eq!(
        read_str(&event.payload, "detail"),
        "Service definition failed to decode: MalformedString { field: \"ImagePath\" }"
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
            released_tty: None,
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

// PEI-368. §10.1: peinit MUST "acknowledge the notification by logging it".
// STOPPING=1 changed peinit's behaviour — it suppresses the SIGTERM — and left
// no trace that it did.
//
// The absence of an action is the one kind of effect that cannot be inferred
// from what happened afterwards. Without a record, a service that was stopping
// and correctly received no SIGTERM looks exactly like a service that should
// have received one and did not: the first is right, the second is a bug in
// peinit, and an operator looking at a service that ran out its StopTimeout
// and got SIGKILLed could not tell them apart.
#[test]
fn a_stopping_notification_is_acknowledged_as_an_event() {
    let sender = AuthenticatedNotifySender {
        service: "app".to_string(),
        job_id: job_id(),
        operation_id: Some(operation_id()),
        generation: 3,
        job_created_at_ns: 1_000,
        cgroup_generation: 7,
    };

    let events = encode_notify_applied_field_events(&sender, &[NotifyAppliedField::Stopping])
        .expect("notify events");

    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["notify.stopping"],
    );
    // It identifies which activation of which service, so an operator can tie
    // it to the stop they are looking at.
    assert_eq!(read_str(&events[0].payload, "service"), "app");
    assert_eq!(
        read_str(&events[0].payload, "job_id"),
        sender.job_id.to_string(),
    );
}

/// READY=1 and RELOADING=1 stay unrecorded, deliberately: both are observable
/// through the state transitions they cause, so an event would be noise.
#[test]
fn ready_and_reloading_are_still_not_separately_recorded() {
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
        &[NotifyAppliedField::Ready, NotifyAppliedField::Reloading],
    )
    .expect("notify events");

    assert!(events.is_empty());
}

/// PEI-1125, PEI-1082: the two audit records that keep a contained failure
/// from reading as the service's own, and a cut or dropped event from
/// leaving a silent gap.
#[test]
fn encodes_internal_error_and_oversized_event_payloads() {
    let dispatch = crate::supervisor::SupervisorInternalErrorDispatch {
        step: "process setup",
        subject: crate::supervisor::SupervisorInternalErrorSubject {
            service: Some("app".to_string()),
            job_id: Some(job_id()),
        },
        error: "Process(\"EBADF\")".to_string(),
        observed_at_ns: OBSERVED_AT_NS,
        job_event: None,
        service_job_event: None,
        operation_event: None,
        service_transition: None,
        start_dispatches: Vec::new(),
    };
    let encoded =
        crate::kmes::encode_service_internal_error_event(&dispatch).expect("encoded event");
    assert_eq!(encoded.event_type, "service.internal_error");
    assert_eq!(read_str(&encoded.payload, "service"), "app");
    assert_eq!(read_str(&encoded.payload, "job_id"), job_id().to_string());
    assert_eq!(read_str(&encoded.payload, "step"), "process setup");
    assert_eq!(read_str(&encoded.payload, "error"), "Process(\"EBADF\")");
    assert_eq!(read_str(&encoded.payload, "service_failed"), "false");
    assert_eq!(
        read_uint(&encoded.payload, "observed_at_ns"),
        OBSERVED_AT_NS
    );
    assert_eq!(
        read_str(&encoded.payload, "message"),
        "peinit: service app: internal error at process setup: Process(\"EBADF\"); the service keeps its state",
    );

    let job = job_id().to_string();
    let oversized = crate::kmes::OversizedEvent {
        event_type: "job.ended",
        action: crate::kmes::OversizedEventAction::Dropped,
        service: None,
        job_id: Some(&job),
        size_bytes: 66_000,
        limit_bytes: None,
        dropped_total: Some(3),
        error: Some("No space left on device (os error 28)"),
    };
    let encoded = crate::kmes::encode_event_oversized_event(&oversized).expect("encoded event");
    assert_eq!(encoded.event_type, "event.oversized");
    assert_eq!(read_str(&encoded.payload, "event"), "job.ended");
    assert_eq!(read_str(&encoded.payload, "action"), "dropped");
    assert_nil(&encoded.payload, "service");
    assert_eq!(read_str(&encoded.payload, "job_id"), job);
    assert_eq!(read_uint(&encoded.payload, "size_bytes"), 66_000);
    assert_nil(&encoded.payload, "limit_bytes");
    assert_eq!(read_uint(&encoded.payload, "dropped_total"), 3);
    assert_eq!(
        read_str(&encoded.payload, "message"),
        format!(
            "event job.ended for job {job} (66000 bytes) was refused by the event ring and dropped: No space left on device (os error 28)"
        ),
    );
    assert_eq!(
        crate::kmes::kmes_event_subject(&encoded.payload),
        (None, Some(job)),
        "the subject of any event can be read back from its payload",
    );
}

fn read_bool(payload: &[u8], field: &str) -> bool {
    read_field(payload, field, |reader| reader.read_bool())
}

/// A finished job record whose `arguments` and the other fields that can be
/// large are whatever the test needs them to be.
fn ended_job_record(
    arguments: Vec<String>,
    token_summary: TokenSummary,
    image_path: String,
    failure_cause: String,
) -> JobRecord {
    JobRecord {
        id: job_id(),
        service: Some("a".repeat(255)),
        job_type: JobType::Submitted,
        hook_index: None,
        state: JobState::Failed,
        pid: Some(u32::MAX),
        pidfd: Some(i32::MAX),
        resolved_identity: "u".repeat(255),
        token_summary,
        required_privileges: Vec::new(),
        image_path,
        arguments,
        environment: Vec::new(),
        working_directory: "/".to_string(),
        limit_nofile: None,
        limit_core: None,
        oom_score_adj: 0,
        created_at_ns: u64::MAX,
        started_at_ns: Some(u64::MAX),
        ended_at_ns: Some(u64::MAX),
        exit_code: Some(i32::MAX),
        exit_signal: Some(i32::MAX),
        failure_cause: Some(failure_cause),
        cgroup_id: "c".repeat(512),
        activation_generation: u64::MAX,
        cgroup_generation: u64::MAX,
        operation_id: Some(operation_id()),
        console_path: None,
    }
}

/// PEI-1082: a `job.ended` carries at most `MAX_JOB_ENDED_ARGUMENTS_BYTES`
/// of `arguments`, whole arguments only, and says how many it left out —
/// so an event PID 1 cannot emit is never built in the first place.
#[test]
fn a_job_ended_event_cuts_its_arguments_to_the_budget_and_says_so() {
    let arguments: Vec<String> = (0..10).map(|_| "a".repeat(4096)).collect();
    let event = JobEvent::ended(&ended_job_record(
        arguments,
        token("SYSTEM"),
        "/bin/true".to_string(),
        "exit 1".to_string(),
    ))
    .expect("job ended event");

    let (encoded, truncation) =
        crate::kmes::encode_job_event_bounded(&event).expect("encoded event");

    // Ten 4 KiB arguments encode to 40,990 bytes (4,099 each); seven of
    // them fit the 32,768-byte budget and the eighth does not.
    let kept = read_str_array(&encoded.payload, "arguments");
    assert_eq!(kept.len(), 7);
    assert!(read_bool(&encoded.payload, "arguments_truncated"));
    assert_eq!(read_uint(&encoded.payload, "arguments_total"), 10);
    assert_eq!(
        truncation,
        Some(crate::kmes::JobEventTruncation {
            arguments_bytes: 10 * (4096 + 3),
            limit_bytes: crate::kmes::MAX_JOB_ENDED_ARGUMENTS_BYTES as u64,
        }),
    );
    assert!(
        encoded.payload.len() < 65_536,
        "{} bytes",
        encoded.payload.len()
    );
}

/// PEI-1082, the defaults: a record that fills a default-sized jobs message
/// produces a `job.ended` that fits a default-sized KMES event with room to
/// spare, however large the record's other fields are — the arguments are
/// never cut, and the whole event stays under the ring's default limit.
/// Both defaults live in this crate and in the kernel respectively:
/// `DEFAULT_MAX_JOBS_MESSAGE_BYTES` (`jobs::socket`, 32768) and
/// `KMES_CONFIG_MAX_EVENT_SIZE_DEFAULT` (`pkm/uapi/pkm/kmes.h`, 65536).
#[test]
fn a_record_at_the_default_message_size_produces_a_job_ended_that_fits_the_default_event_size() {
    const KMES_DEFAULT_MAX_EVENT_SIZE: usize = 65_536;
    const DEFAULT_MAX_JOBS_MESSAGE_BYTES: usize =
        crate::jobs::socket::DEFAULT_MAX_JOBS_MESSAGE_BYTES;
    const {
        assert!(DEFAULT_MAX_JOBS_MESSAGE_BYTES <= crate::kmes::MAX_JOB_ENDED_ARGUMENTS_BYTES);
        assert!(2 * crate::kmes::MAX_JOB_ENDED_ARGUMENTS_BYTES <= KMES_DEFAULT_MAX_EVENT_SIZE);
    }

    // The whole message budget spent on one argument, beside a token with
    // 128 groups and 64 privileges of each kind, a PATH_MAX image path and
    // a 4 KiB failure cause. The argument is the message less the smallest
    // record that can carry it: a MessagePack string costs at most three
    // bytes over its length, a JSON one at least two plus its framing, so
    // the encoded arguments of any record are smaller than the record.
    const SMALLEST_SUBMIT_RECORD: usize =
        r#"{"command":"submit","image_path":"/","arguments":[""]}"#.len();
    let mut token_summary = token(&"i".repeat(255));
    token_summary.user_sid = "S-1-5-21-".to_string() + &"9".repeat(180);
    token_summary.group_sids = (0..128)
        .map(|index| format!("S-1-5-21-{}-{index}", "9".repeat(50)))
        .collect();
    token_summary.present_privileges = (0..64)
        .map(|index| format!("SePrivilege{index:02}"))
        .collect();
    token_summary.enabled_privileges = token_summary.present_privileges.clone();
    let event = JobEvent::ended(&ended_job_record(
        vec!["a".repeat(DEFAULT_MAX_JOBS_MESSAGE_BYTES - SMALLEST_SUBMIT_RECORD)],
        token_summary,
        "/".to_string() + &"p".repeat(4095),
        "f".repeat(4096),
    ))
    .expect("job ended event");

    let (encoded, truncation) =
        crate::kmes::encode_job_event_bounded(&event).expect("encoded event");

    assert_eq!(truncation, None);
    assert!(!read_bool(&encoded.payload, "arguments_truncated"));
    // Header plus payload is what the ring counts; the header and the
    // event type are well under a kilobyte.
    assert!(
        encoded.payload.len() + 1024 <= KMES_DEFAULT_MAX_EVENT_SIZE,
        "{} bytes",
        encoded.payload.len()
    );
}
