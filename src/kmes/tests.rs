use std::str::FromStr;

use peios::msgpack::{Reader, Type};

use crate::boot::phase2::{BlockedReason, Phase2BootPlanError, Phase2BootRunError};
use crate::boundary::{EventTier, EventTimeProjection, KmesEvent};
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
    CriticalRebootTrigger, SupervisorError, SupervisorFdStoreRejectionDispatch,
    SupervisorOnFailureLoopSuppressedDispatch, SupervisorOnFailureLoopSuppressionReason,
    SupervisorShutdownAbandonedDispatch, SupervisorShutdownFinalizationDispatch,
};

use super::types::{EVENT_TYPES, tier_of};
use super::{
    DroppedEvent, EventCollector, GraphPhase, KmesEventSubject, NotifyRejectionReason,
    collect_init_recovery_events, dropped_event_message, encode_boot_blocked_service_event,
    encode_config_reload_applied_event, encode_critical_failure_event, encode_event_dropped_event,
    encode_fd_store_rejection_event, encode_graph_event, encode_graph_validation_error_event,
    encode_graph_validation_warning_event, encode_job_event, encode_job_status_event,
    encode_leaked_job_cgroup_event, encode_notify_field_event, encode_notify_progress_event,
    encode_notify_rejection_event, encode_on_failure_loop_suppressed_event,
    encode_operation_event, encode_output_dropped_event, encode_registry_reload_deferred_event,
    encode_reload_undecodable_service_event, encode_reload_unconfirmed_event,
    encode_shutdown_abandoned_event, kmes_event_subject, notify_field_event_type,
};

const OBSERVED_AT_NS: u64 = 1_717_171_717_123_456_789;

/// Monotonic 1,000 is the wall clock's 1,700,000,000,000,001,000.
const TIME: EventTimeProjection = EventTimeProjection::new(1_000, 1_700_000_000_000_001_000);

#[test]
fn a_job_ended_is_the_whole_job_record_nested_under_its_catalogue_paths() {
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
        token_summary: system_token(),
        required_privileges: Vec::new(),
        image_path: "/sbin/app".to_string(),
        arguments: vec!["--foreground".to_string(), "--ready".to_string()],
        environment: Vec::new(),
        working_directory: "/".to_string(),
        limit_nofile: None,
        limit_core: None,
        oom_score_adj: 0,
        created_at_ns: 400,
        started_at_ns: Some(600),
        ended_at_ns: Some(1_000),
        exit_code: Some(2),
        exit_signal: None,
        failure_cause: Some("exit-code".to_string()),
        cgroup_id: "/sys/fs/cgroup/peinit/app".to_string(),
        activation_generation: 3,
        cgroup_generation: 7,
        operation_id: Some(operation_id),
        console_path: None,
    })
    .expect("job ended event");

    let encoded = encode_job_event(&event, TIME).expect("encoded event");
    let p = &encoded.payload;

    assert_eq!(encoded.event_type, "peinit.job.ended");
    assert_eq!(read_bin(p, "object.job.guid"), job_id.as_guid_bytes());
    assert_eq!(read_str(p, "object.job.type"), "service-main");
    assert_eq!(read_str(p, "object.job.state"), "failed");
    assert_eq!(read_str(p, "object.service.name"), "app");
    assert_eq!(read_uint(p, "object.job.activation-generation"), 3);
    assert_eq!(read_bin(p, "object.operation.guid"), operation_id.as_guid_bytes());
    assert!(!read_bool(p, "outcome.success"));
    assert_eq!(read_str(p, "outcome.detail"), "exit-code");
    assert_eq!(read_uint(p, "object.process.pid"), 123);
    assert_eq!(read_int(p, "object.process.exit-code"), 2);
    assert_absent(p, "object.process.exit-signal");
    assert_eq!(read_str(p, "object.job.executable"), "/sbin/app");
    assert_eq!(read_str_array(p, "object.job.arguments"), ["--foreground", "--ready"]);
    assert!(!read_bool(p, "object.job.arguments-truncated"));
    assert_eq!(read_uint(p, "object.job.arguments-count"), 2);
    // Monotonic instants are written as the wall-clock times they were.
    assert_eq!(read_uint(p, "object.job.created-time"), 1_700_000_000_000_000_400);
    assert_eq!(read_uint(p, "object.job.started-time"), 1_700_000_000_000_000_600);
    assert_eq!(read_uint(p, "object.job.duration"), 600);
    assert_eq!(read_str(p, "object.cgroup.path"), "/sys/fs/cgroup/peinit/app");
    assert_eq!(read_uint(p, "object.cgroup.generation"), 7);
    assert_eq!(read_bin(p, "object.job.token.sid"), sid("S-1-5-18"));
    assert_eq!(
        read_bin_array(p, "object.job.token.groups"),
        [sid("S-1-5-32-544"), sid("S-1-1-0")]
    );
    let tcb = peios::security::Privileges::TCB.bits();
    let audit = peios::security::Privileges::AUDIT.bits();
    assert_eq!(read_uint(p, "object.job.token.privileges"), tcb | audit);
    assert_eq!(read_uint(p, "object.job.token.privileges-enabled"), tcb);
    // Names are never carried, and nothing is written as nil.
    for gone in ["job_id", "pidfd", "resolved_identity", "token_identity", "final_state"] {
        assert_absent(p, gone);
    }
    assert_no_nil(p);
    assert_eq!(
        top_level_keys(p),
        ["object", "outcome"],
        "every field is under its participant or its outcome"
    );
}

#[test]
fn a_completed_job_has_no_failure_detail_and_a_submitted_job_no_service() {
    let mut record = ended_job_record(
        vec!["true".to_string()],
        system_token(),
        "/bin/true".to_string(),
        "should not be written".to_string(),
    );
    record.state = JobState::Completed;
    record.service = None;
    let event = JobEvent::ended(&record).expect("job ended event");

    let encoded = encode_job_event(&event, TIME).expect("encoded event");
    let p = &encoded.payload;
    assert!(read_bool(p, "outcome.success"));
    assert_absent(p, "outcome.detail");
    assert_eq!(read_str(p, "object.job.type"), "submitted");
    // A submitted job belongs to no service, and its generation of zero is
    // not a generation at all.
    assert_absent(p, "object.service");
    assert_absent(p, "object.job.activation-generation");
}

#[test]
fn a_job_created_and_started_carry_the_job_and_what_they_add() {
    let mut record = ended_job_record(
        vec!["app".to_string()],
        system_token(),
        "/sbin/app".to_string(),
        String::new(),
    );
    record.job_type = JobType::PreExecHook;
    record.service = Some("app".to_string());
    record.state = JobState::Created;
    record.activation_generation = 4;

    let created = encode_job_event(&JobEvent::created(&record), TIME).expect("created");
    assert_eq!(created.event_type, "peinit.job.created");
    assert_eq!(read_str(&created.payload, "object.job.type"), "pre-exec-hook");
    assert_eq!(read_str(&created.payload, "object.job.state"), "created");
    assert_eq!(read_str(&created.payload, "object.job.executable"), "/sbin/app");
    assert_eq!(read_uint(&created.payload, "object.job.activation-generation"), 4);
    assert_absent(&created.payload, "object.process");
    assert_no_nil(&created.payload);

    record.state = JobState::Running;
    record.pid = Some(321);
    let started = encode_job_event(
        &JobEvent::started(&record).expect("started event"),
        TIME,
    )
    .expect("started");
    assert_eq!(started.event_type, "peinit.job.started");
    assert_eq!(read_uint(&started.payload, "object.process.pid"), 321);
    assert_eq!(read_str(&started.payload, "object.cgroup.path"), "c".repeat(512));
    assert_absent(&started.payload, "started_at_ns");
    assert_no_nil(&started.payload);
}

#[test]
fn every_way_an_operation_ends_is_one_type_told_apart_by_its_state() {
    let operation_id = operation_id();
    let record = |state, result: Option<&str>, started| OperationRecord {
        id: operation_id,
        operation_type: OperationType::Start,
        service: "app".to_string(),
        state,
        created_at_ns: 10,
        lifetime_from_ns: 10,
        started_at_ns: started,
        completed_at_ns: Some(45),
        source: OperationSource::DependencyPropagation,
        caller: Some(caller_token()),
        result: result.map(str::to_string),
        merged_into: None,
        service_security: None,
    };

    let completed = encode_operation_event(
        &OperationEvent::completed(&record(OperationState::Completed, Some("started"), Some(12)))
            .expect("completed"),
    )
    .expect("encoded");
    let p = &completed.payload;
    assert_eq!(completed.event_type, "peinit.operation.ended");
    assert_eq!(read_bin(p, "object.operation.guid"), operation_id.as_guid_bytes());
    assert_eq!(read_str(p, "object.operation.type"), "start");
    assert_eq!(read_str(p, "object.operation.source"), "dependency-propagation");
    assert_eq!(read_str(p, "object.operation.state"), "completed");
    assert_eq!(read_str(p, "object.service.name"), "app");
    assert!(read_bool(p, "outcome.success"));
    assert_eq!(read_str(p, "outcome.detail"), "started");
    assert_eq!(read_uint(p, "object.operation.duration"), 35);
    // The client who asked, by SID alone: peinit knows no more of a
    // control-channel caller's token, so it writes no more.
    assert_eq!(read_bin(p, "subject.token.sid"), sid("S-1-5-21-1-2-3-1001"));
    assert_absent(p, "subject.token.groups");
    assert_absent(p, "caller");
    assert_no_nil(p);

    let failed = encode_operation_event(
        &OperationEvent::failed(&record(OperationState::Failed, Some("inactive"), Some(12)))
            .expect("failed"),
    )
    .expect("encoded");
    assert_eq!(failed.event_type, "peinit.operation.ended");
    assert!(!read_bool(&failed.payload, "outcome.success"));
    assert_eq!(read_str(&failed.payload, "object.operation.state"), "failed");
    assert_eq!(read_str(&failed.payload, "outcome.detail"), "inactive");
}

#[test]
fn an_operation_requested_by_peinit_itself_has_no_subject() {
    let event = OperationEvent::requested(&OperationRecord {
        id: operation_id(),
        operation_type: OperationType::Restart,
        service: "app".to_string(),
        state: OperationState::Pending,
        created_at_ns: 10,
        lifetime_from_ns: 10,
        started_at_ns: None,
        completed_at_ns: None,
        source: OperationSource::RestartPolicy,
        caller: None,
        result: None,
        merged_into: None,
        service_security: None,
    });
    let encoded = encode_operation_event(&event).expect("encoded");
    assert_eq!(encoded.event_type, "peinit.operation.requested");
    assert_eq!(read_str(&encoded.payload, "object.operation.state"), "pending");
    assert_eq!(read_str(&encoded.payload, "object.operation.source"), "restart-policy");
    assert_absent(&encoded.payload, "subject");
    assert_absent(&encoded.payload, "outcome");
}

#[test]
fn a_graph_member_reaching_its_outcome_is_a_graph_operation_ended() {
    let operation_id = operation_id();
    let event = encode_graph_event(&crate::execution::graph::GraphExecutionEvent {
        context_id: crate::execution::graph::GraphContextId::new_for_test(4),
        service: "db".to_string(),
        operation_id,
        outcome: crate::execution::graph::GraphTerminalOutcome::Satisfied,
    })
    .expect("graph event");
    assert_eq!(event.event_type, "peinit.graph.operation.ended");
    assert_eq!(read_uint(&event.payload, "graph.context"), 4);
    assert_eq!(read_str(&event.payload, "object.service.name"), "db");
    assert_eq!(read_bin(&event.payload, "object.operation.guid"), operation_id.as_guid_bytes());
    assert!(read_bool(&event.payload, "outcome.success"));
}

#[test]
fn notify_fields_are_recorded_with_their_sender_and_their_value_typed() {
    let sender = sender(Some(operation_id()));

    let fields = [
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
        NotifyAppliedField::Stopping,
    ];
    let events: Vec<KmesEvent> = fields
        .iter()
        .filter(|field| notify_field_event_type(field).is_some())
        .map(|field| encode_notify_field_event(&sender, field).expect("notify event"))
        .collect();

    assert_eq!(
        events.iter().map(|event| event.event_type.as_str()).collect::<Vec<_>>(),
        [
            "peinit.notify.status.reported",
            "peinit.notify.errno.reported",
            "peinit.notify.exit-status.reported",
            "peinit.notify.stopping.reported",
        ]
    );
    let status = &events[0].payload;
    assert_eq!(read_str(status, "subject.service.name"), "app");
    assert_eq!(read_bin(status, "subject.job.guid"), sender.job_id.as_guid_bytes());
    assert_eq!(read_uint(status, "subject.job.activation-generation"), 3);
    assert_eq!(
        read_bin(status, "subject.operation.guid"),
        sender.operation_id.expect("operation").as_guid_bytes()
    );
    assert_eq!(read_str(status, "notify.status"), "warming cache");
    // ERRNO= is sent positive and carried negated, as every errno is.
    assert_eq!(read_int(&events[1].payload, "notify.errno"), -5);
    assert_eq!(read_int(&events[2].payload, "notify.exit-status"), 75);
    // STOPPING=1 is recorded because its only effect is an absence: no
    // SIGTERM (PEI-368). The sender is the whole record.
    assert_eq!(top_level_keys(&events[3].payload), ["subject"]);
}

#[test]
fn a_notify_value_that_is_not_a_number_is_left_out_and_the_event_kept() {
    let sender = sender(None);
    let errno = encode_notify_field_event(
        &sender,
        &NotifyAppliedField::Errno {
            value: "EIO".to_string(),
        },
    )
    .expect("errno event");
    assert_absent(&errno.payload, "notify");
    assert_absent(&errno.payload, "subject.operation");
    let exit = encode_notify_field_event(
        &sender,
        &NotifyAppliedField::ExitStatus {
            value: "seventy-five".to_string(),
        },
    )
    .expect("exit status event");
    assert_absent(&exit.payload, "notify");
}

/// READY=1 and RELOADING=1 stay unrecorded, deliberately: both are observable
/// through the state transitions they cause, so an event would be noise.
#[test]
fn ready_reloading_and_progress_fields_have_no_event_of_their_own() {
    for field in [
        NotifyAppliedField::Ready,
        NotifyAppliedField::Reloading,
        NotifyAppliedField::Progress {
            value: "3/10".to_string(),
        },
    ] {
        assert_eq!(notify_field_event_type(&field), None, "{field:?}");
    }
}

/// PSPU §4.19: a service's PROGRESS is recorded as one structured event
/// carrying the retained value, and `notify.progress.bounded` is what tells
/// `PROGRESS=N` from `PROGRESS=N/`.
#[test]
fn a_services_progress_is_recorded_as_an_event() {
    use crate::service::runtime::ServiceProgressReport;
    use crate::submitted::{JobProgress, JobProgressUnit};

    let sender = sender(None);
    let event = encode_notify_progress_event(
        &sender,
        &ServiceProgressReport {
            progress: Some(JobProgress {
                current: 3,
                total: Some(10),
                bounded: true,
            }),
            unit: Some(JobProgressUnit::Items),
        },
    )
    .expect("progress event");
    let p = &event.payload;
    assert_eq!(event.event_type, "peinit.notify.progress.reported");
    assert_eq!(read_str(p, "subject.service.name"), "app");
    assert_absent(p, "subject.operation");
    assert_eq!(read_uint(p, "notify.progress.current"), 3);
    assert_eq!(read_uint(p, "notify.progress.total"), 10);
    assert!(read_bool(p, "notify.progress.bounded"));
    assert_eq!(read_str(p, "notify.progress.unit"), "items");

    // PROGRESS=N/: an end declared and not yet known.
    let open = encode_notify_progress_event(
        &sender,
        &ServiceProgressReport {
            progress: Some(JobProgress {
                current: 3,
                total: None,
                bounded: true,
            }),
            unit: None,
        },
    )
    .expect("open progress event");
    assert!(read_bool(&open.payload, "notify.progress.bounded"));
    assert_absent(&open.payload, "notify.progress.total");
    assert_absent(&open.payload, "notify.progress.unit");

    // A unit alone: no figure yet, so no figure is written.
    let unit = encode_notify_progress_event(
        &sender,
        &ServiceProgressReport {
            progress: None,
            unit: Some(JobProgressUnit::Bytes),
        },
    )
    .expect("unit-only progress event");
    assert_absent(&unit.payload, "notify.progress.current");
    assert_absent(&unit.payload, "notify.progress.bounded");
    assert_eq!(read_str(&unit.payload, "notify.progress.unit"), "bytes");
    assert_no_nil(&unit.payload);
}

#[test]
fn a_submitted_jobs_status_names_its_submitter_by_sid() {
    use crate::submitted::{JobProgress, JobProgressUnit};

    let job_id = job_id();
    let event = encode_job_status_event(&crate::supervisor::SupervisorSubmittedNotifyDispatch {
        job_id,
        submitter_sid: "S-1-5-21-1-2-3-1001".to_string(),
        applied: Vec::new(),
        status_text: Some("copying".to_string()),
        progress: Some(JobProgress {
            current: 1,
            total: Some(4),
            bounded: true,
        }),
        progress_unit: Some(JobProgressUnit::Percent),
        status_event_due: true,
    })
    .expect("status event");
    let p = &event.payload;
    assert_eq!(event.event_type, "peinit.job.status.reported");
    assert_eq!(read_bin(p, "object.job.guid"), job_id.as_guid_bytes());
    assert_eq!(read_bin(p, "object.job.submitter.sid"), sid("S-1-5-21-1-2-3-1001"));
    assert_eq!(read_str(p, "notify.status"), "copying");
    assert_eq!(read_uint(p, "notify.progress.total"), 4);
    assert_eq!(read_str(p, "notify.progress.unit"), "percent");

    let dropped = encode_output_dropped_event(job_id).expect("output dropped");
    assert_eq!(dropped.event_type, "peinit.job.output.dropped");
    assert_eq!(top_level_keys(&dropped.payload), ["object"]);
    assert_absent(&dropped.payload, "message");
}

#[test]
fn encodes_fd_store_rejection_payload() {
    let event = encode_fd_store_rejection_event(&SupervisorFdStoreRejectionDispatch {
        service: "app".to_string(),
        name: "api".to_string(),
        outcome: StoreFdOutcome::Full,
    })
    .expect("fd-store rejection event");

    assert_eq!(event.event_type, "peinit.fd-store.rejected");
    assert_eq!(read_str(&event.payload, "object.service.name"), "app");
    assert_eq!(read_str(&event.payload, "object.fd-store.name"), "api");
    assert_eq!(read_str(&event.payload, "outcome.reason"), "full");
    assert_absent(&event.payload, "reason");
}

#[test]
fn a_notify_rejection_names_its_reason_and_whoever_peinit_could_attribute() {
    let sender = sender(None);
    let attributed = encode_notify_rejection_event(
        Some(55),
        NotifyRejectionReason::GenerationMismatch,
        Some(&sender),
    )
    .expect("notify rejection event");
    let p = &attributed.payload;
    assert_eq!(attributed.event_type, "peinit.notify.rejected");
    assert_eq!(read_str(p, "outcome.reason"), "generation-mismatch");
    assert_eq!(read_uint(p, "subject.process.pid"), 55);
    assert_eq!(read_str(p, "subject.service.name"), "app");
    assert_eq!(read_bin(p, "subject.job.guid"), sender.job_id.as_guid_bytes());
    assert_absent(p, "outcome.success");

    let anonymous = encode_notify_rejection_event(None, NotifyRejectionReason::Truncated, None)
        .expect("anonymous rejection");
    assert_eq!(top_level_keys(&anonymous.payload), ["outcome"]);
    assert_eq!(read_str(&anonymous.payload, "outcome.reason"), "truncated");
}

#[test]
fn every_notify_rejection_reason_is_one_the_fragment_declares() {
    let fragment = include_str!("../../peinit.evman");
    let declared = declared_values(fragment, "peinit.notify.rejected", "outcome.reason");
    use NotifyRejectionReason::*;
    for reason in [
        InvalidUtf8,
        MalformedLine,
        Truncated,
        UnauthenticatedSender,
        MissingService,
        JobNotRunning,
        MissingProcess,
        PidfdMismatch,
        ProcessVerificationFailed,
        GenerationMismatch,
        MissingStartOperation,
        MissingReloadOperation,
        UnsupportedReloadOperation,
        UnsupportedReadyState,
        ShutdownRefused,
        InternalError,
    ] {
        assert!(declared.contains(&reason.label().to_string()), "{}", reason.label());
    }
}

#[test]
fn graph_validation_findings_and_warnings_carry_their_reason_and_subjects() {
    let warning = encode_graph_validation_warning_event(
        GraphPhase::ReloadConfig,
        &ServiceGraphWarning::AliveReadinessWithHardDependents {
            service: "db".to_string(),
            dependents: vec!["app".to_string()],
        },
    )
    .expect("graph warning event");
    assert_eq!(warning.event_type, "peinit.graph.validation.warned");
    assert_eq!(read_str(&warning.payload, "graph.phase"), "reload-config");
    assert_eq!(
        read_str(&warning.payload, "outcome.reason"),
        "alive-readiness-with-hard-dependents"
    );
    assert_eq!(read_str(&warning.payload, "object.service.name"), "db");
    assert_eq!(read_str_array(&warning.payload, "object.service.dependents"), ["app"]);
    assert_absent(&warning.payload, "message");

    let role = encode_graph_validation_warning_event(
        GraphPhase::Boot,
        &ServiceGraphWarning::UnfilledRole {
            role: "authn".to_string(),
            services: vec!["login".to_string()],
        },
    )
    .expect("role warning");
    assert_eq!(read_str(&role.payload, "outcome.reason"), "unfilled-role");
    assert_eq!(read_str(&role.payload, "graph.role"), "authn");
    assert_eq!(read_str_array(&role.payload, "graph.services"), ["login"]);

    let error = encode_graph_validation_error_event(
        GraphPhase::ReloadConfig,
        &ServiceGraphFinding::MissingHardDependency {
            service: "app".to_string(),
            target: "db".to_string(),
            kind: ServiceDependencyKind::BindsTo,
        },
    )
    .expect("graph error event");
    assert_eq!(error.event_type, "peinit.graph.validation.failed");
    assert_eq!(read_str(&error.payload, "outcome.reason"), "missing-hard-dependency");
    assert_eq!(read_str(&error.payload, "object.service.dependency.name"), "db");
    assert_eq!(read_str(&error.payload, "object.service.dependency.kind"), "binds-to");

    let timer = encode_graph_validation_error_event(
        GraphPhase::ReloadConfig,
        &ServiceGraphFinding::InvalidTimerSchedule {
            service: "timer".to_string(),
            schedule: "*-*-* 12:00:00.5 UTC".to_string(),
            message: "fractional seconds are not supported".to_string(),
        },
    )
    .expect("timer graph error event");
    assert_eq!(read_str(&timer.payload, "outcome.reason"), "invalid-timer-schedule");
    assert_eq!(
        read_str(&timer.payload, "object.service.timer.schedule"),
        "*-*-* 12:00:00.5 UTC"
    );
    assert_eq!(
        read_str(&timer.payload, "outcome.detail"),
        "fractional seconds are not supported"
    );

    // Configured in seconds, carried as `uint.duration` nanoseconds.
    let health = encode_graph_validation_error_event(
        GraphPhase::ReloadConfig,
        &ServiceGraphFinding::InvalidHealthCheckRestartWindow {
            service: "web".to_string(),
            retries: 3,
            interval_secs: 10,
            restart_window_secs: 20,
        },
    )
    .expect("health graph error event");
    assert_eq!(
        read_uint(&health.payload, "object.service.health-check.interval"),
        10_000_000_000
    );
    assert_eq!(read_uint(&health.payload, "object.service.health-check.retries"), 3);
    assert_eq!(
        read_uint(&health.payload, "object.service.health-check.restart-window"),
        20_000_000_000
    );
}

/// PEI-350, and Q9: the boot window's deferral; the reload that applies it,
/// which alone names what was deferred; and an explicit `reload-config`,
/// which records that it applied too.
#[test]
fn a_reload_records_that_it_applied_with_counts_only_on_success() {
    let deferred = encode_registry_reload_deferred_event(&["app".to_string(), "db".to_string()])
        .expect("deferred event");
    assert_eq!(deferred.event_type, "peinit.config.reload.deferred");
    assert_eq!(read_str_array(&deferred.payload, "graph.services"), ["app", "db"]);
    assert_eq!(top_level_keys(&deferred.payload), ["graph"]);

    let outcome = reload_outcome();
    let coalesced = encode_config_reload_applied_event(
        Some(&["app".to_string(), "db".to_string()]),
        Ok(&outcome),
    )
    .expect("coalesced event");
    let p = &coalesced.payload;
    assert_eq!(coalesced.event_type, "peinit.config.reload.applied");
    assert!(read_bool(p, "outcome.success"));
    assert_eq!(read_str_array(p, "graph.services"), ["app", "db"]);
    assert_eq!(read_uint(p, "graph.counts.added"), 1);
    assert_eq!(read_uint(p, "graph.counts.updated"), 0);
    assert_eq!(read_uint(p, "graph.counts.undecodable"), 1);

    let explicit =
        encode_config_reload_applied_event(None, Ok(&outcome)).expect("explicit reload event");
    assert_absent(&explicit.payload, "graph.services");
    assert_eq!(read_uint(&explicit.payload, "graph.counts.marked-removed"), 0);

    let failed = encode_config_reload_applied_event(
        None,
        Err(&crate::control::reload_config::ReloadConfigError::Registry(
            crate::boundary::BoundaryError::Registry("offline".to_string()),
        )),
    )
    .expect("failed reload event");
    assert!(!read_bool(&failed.payload, "outcome.success"));
    // The registry's own words, never Rust Debug output.
    assert_eq!(read_str(&failed.payload, "outcome.detail"), "offline");
    // A failed reload changed nothing, so it counts nothing.
    assert_absent(&failed.payload, "graph");
}

/// PEI-621: a key a reload could not decode is audited as the same
/// `validation-error` finding the boot records for one, under the
/// `reload-config` phase.
#[test]
fn encodes_reload_undecodable_service_as_a_validation_error() {
    let event = encode_reload_undecodable_service_event(&crate::boundary::UndecodableService {
        name: "broken".to_string(),
        field: Some("ImagePath".to_string()),
        message: "MalformedString { field: \"ImagePath\" }".to_string(),
    })
    .expect("undecodable event");

    assert_eq!(event.event_type, "peinit.graph.validation.failed");
    assert_eq!(read_str(&event.payload, "graph.phase"), "reload-config");
    assert_eq!(read_str(&event.payload, "outcome.reason"), "validation-error");
    assert_eq!(read_str(&event.payload, "object.service.name"), "broken");
    assert_eq!(
        read_str(&event.payload, "outcome.detail"),
        "Service definition failed to decode: MalformedString { field: \"ImagePath\" }"
    );
}

#[test]
fn encodes_on_failure_loop_suppression_payload() {
    let event =
        encode_on_failure_loop_suppressed_event(&SupervisorOnFailureLoopSuppressedDispatch {
            failed_service: "a".to_string(),
            attempted_handler: "b".to_string(),
            chain: vec!["b".to_string(), "a".to_string(), "b".to_string()],
            reason: SupervisorOnFailureLoopSuppressionReason::MaxDepth { max_depth: 16 },
        })
        .expect("on-failure loop suppression event");

    assert_eq!(event.event_type, "peinit.on-failure.suppressed");
    assert_eq!(read_str(&event.payload, "object.service.name"), "a");
    assert_eq!(read_str(&event.payload, "object.service.on-failure.name"), "b");
    assert_eq!(
        read_str_array(&event.payload, "object.service.on-failure-chain"),
        ["b", "a", "b"]
    );
    assert_eq!(read_str(&event.payload, "outcome.reason"), "max-depth");
}

#[test]
fn recovery_writes_its_reason_and_the_plans_finding_with_no_debug_text() {
    let mut out = EventCollector::everything(TIME);
    collect_init_recovery_events(
        &InitRecoveryReason::Phase2(SupervisorError::Phase2Boot(Phase2BootRunError::Plan(
            Phase2BootPlanError::Cycle {
                services: vec!["a".to_string(), "b".to_string(), "a".to_string()],
            },
        ))),
        &mut out,
    )
    .expect("recovery events");
    let events = out.into_events();

    assert_eq!(
        events.iter().map(|event| event.event_type.as_str()).collect::<Vec<_>>(),
        ["peinit.recovery.entered", "peinit.graph.validation.failed"],
    );
    assert_eq!(read_str(&events[0].payload, "outcome.reason"), "phase2");
    assert_absent(&events[0].payload, "outcome.detail");
    assert_eq!(read_str(&events[1].payload, "graph.phase"), "phase2-boot");
    assert_eq!(read_str(&events[1].payload, "outcome.reason"), "cycle");
    assert_eq!(read_str_array(&events[1].payload, "graph.services"), ["a", "b", "a"]);

    let mut out = EventCollector::everything(TIME);
    collect_init_recovery_events(
        &InitRecoveryReason::Privileges(crate::boundary::BoundaryError::Token(
            "peinit is missing required privilege(s): SeAuditPrivilege".to_string(),
        )),
        &mut out,
    )
    .expect("recovery events");
    let events = out.into_events();
    assert_eq!(events.len(), 1);
    assert_eq!(read_str(&events[0].payload, "outcome.reason"), "privileges");
    assert_eq!(
        read_str(&events[0].payload, "outcome.detail"),
        "peinit is missing required privilege(s): SeAuditPrivilege"
    );
}

/// The recovery record is essential and written whatever the policy says;
/// the plan's finding is standard and is not.
#[test]
fn a_policy_that_switches_everything_off_still_gets_the_recovery_record() {
    let off = |_: &str, _: EventTier| false;
    let mut out = EventCollector::new(&off, TIME);
    collect_init_recovery_events(
        &InitRecoveryReason::Phase2(SupervisorError::Phase2Boot(Phase2BootRunError::Plan(
            Phase2BootPlanError::DuplicateService {
                service: "a".to_string(),
            },
        ))),
        &mut out,
    )
    .expect("recovery events");
    assert_eq!(
        out.events().iter().map(|event| event.event_type.as_str()).collect::<Vec<_>>(),
        ["peinit.recovery.entered"]
    );
}

#[test]
fn encodes_shutdown_abandonment_and_critical_failure() {
    let abandoned = encode_shutdown_abandoned_event(&SupervisorShutdownAbandonedDispatch {
        service: "app".to_string(),
        cgroup_id: "/sys/fs/cgroup/peinit/app".to_string(),
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
    let p = &abandoned.payload;
    assert_eq!(abandoned.event_type, "peinit.service.abandoned");
    assert_eq!(read_str(p, "object.service.name"), "app");
    assert_eq!(read_str(p, "object.cgroup.path"), "/sys/fs/cgroup/peinit/app");
    assert_eq!(read_str(p, "object.service.state-previous"), "stopping");
    assert_eq!(read_str(p, "object.service.state"), "abandoned");
    assert_eq!(read_str(p, "object.service.transition-cause"), "process-unkillable");
    assert_eq!(read_uint(p, "object.service.generation"), 6);

    let critical = encode_critical_failure_event(
        "app",
        CriticalRebootTrigger::WatchdogTimeout,
        &SupervisorShutdownFinalizationDispatch {
            report: empty_finalization_report(),
            finalization: ShutdownFinalizationState::WaitingForServices,
        },
    )
    .expect("critical failure event");
    let p = &critical.payload;
    assert_eq!(critical.event_type, "peinit.critical-service.failed");
    assert_eq!(read_str(p, "object.service.name"), "app");
    assert_eq!(read_str(p, "outcome.reason"), "watchdog-timeout");
    assert_eq!(read_str(p, "shutdown.finalization-state"), "waiting-for-services");
    assert_absent(p, "outcome.detail");
    for gone in ["final_action", "observed_at_ns", "trigger"] {
        assert_absent(p, gone);
    }
}

#[test]
fn a_reload_left_unconfirmed_and_a_leaked_job_cgroup_name_what_they_are_about() {
    let timed_out = encode_reload_unconfirmed_event("app").expect("reload timed out");
    assert_eq!(timed_out.event_type, "peinit.service.reload.timed-out");
    assert_eq!(top_level_keys(&timed_out.payload), ["object"]);
    assert_eq!(read_str(&timed_out.payload, "object.service.name"), "app");

    let job_id = job_id();
    let leaked =
        encode_leaked_job_cgroup_event(job_id, "/sys/fs/cgroup/peinit/jobs/x").expect("leak");
    assert_eq!(leaked.event_type, "peinit.cgroup.leaked");
    // A submitted job's cgroup belongs to the job, not to a service.
    assert_eq!(read_bin(&leaked.payload, "object.job.guid"), job_id.as_guid_bytes());
    assert_absent(&leaked.payload, "object.service");
    assert_eq!(read_str(&leaked.payload, "object.cgroup.type"), "service-tree");
}

/// Boot findings reuse `peinit.graph.validation.failed` and are told apart
/// from reload findings by `graph.phase`, so one filter catches both.
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

    assert_eq!(missing.event_type, "peinit.graph.validation.failed");
    assert_eq!(read_str(&missing.payload, "graph.phase"), "boot");
    assert_eq!(read_str(&missing.payload, "outcome.reason"), "missing-hard-dependency");
    assert_eq!(read_str(&missing.payload, "object.service.name"), "app");
    assert_eq!(read_str(&missing.payload, "object.service.dependency.name"), "db");
    assert_eq!(read_str(&missing.payload, "object.service.dependency.kind"), "requires");

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
    assert_eq!(read_str(&blocked.payload, "outcome.reason"), "hard-dependency-blocked");

    let conflict = encode_boot_blocked_service_event(
        "a",
        &BlockedReason::ConflictingBootService {
            target: "b".to_string(),
        },
    )
    .expect("conflict event");
    assert_eq!(read_str(&conflict.payload, "outcome.reason"), "conflicting-boot-services");
    assert_eq!(read_str(&conflict.payload, "object.service.conflict.name"), "b");

    let validation = encode_boot_blocked_service_event(
        "app",
        &BlockedReason::ValidationError {
            message: "health check interval exceeds the restart window".to_string(),
        },
    )
    .expect("validation event");
    assert_eq!(read_str(&validation.payload, "outcome.reason"), "validation-error");
    assert_eq!(
        read_str(&validation.payload, "outcome.detail"),
        "health check interval exceeds the restart window"
    );
}

/// PEI-1125, PEI-1082: the two records that keep a contained failure from
/// reading as the service's own, and a dropped event from leaving a silent
/// gap.
#[test]
fn encodes_internal_error_and_dropped_event_payloads() {
    let job_id = job_id();
    let dispatch = crate::supervisor::SupervisorInternalErrorDispatch {
        step: "process setup",
        subject: crate::supervisor::SupervisorInternalErrorSubject {
            service: Some("app".to_string()),
            job_id: Some(job_id),
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
    let p = &encoded.payload;
    assert_eq!(encoded.event_type, "peinit.internal-error.contained");
    assert_eq!(read_str(p, "operation.stage"), "process-setup");
    assert_eq!(read_str(p, "object.service.name"), "app");
    assert!(!read_bool(p, "object.service.failed"));
    assert_eq!(read_bin(p, "object.job.guid"), job_id.as_guid_bytes());
    // The error is Rust Debug text, which an event never carries.
    assert_absent(p, "outcome");
    for gone in ["observed_at_ns", "message", "error", "step"] {
        assert_absent(p, gone);
    }

    let subject = KmesEventSubject {
        service: None,
        job_guid: Some(job_id.as_guid_bytes()),
    };
    let dropped = DroppedEvent {
        event_type: "peinit.job.ended",
        subject: &subject,
        payload_length: 66_000,
        errno: Some(28),
    };
    let encoded = encode_event_dropped_event(&dropped).expect("encoded event");
    let p = &encoded.payload;
    assert_eq!(encoded.event_type, "peinit.event.dropped");
    assert_eq!(read_str(p, "emission.type"), "peinit.job.ended");
    assert_eq!(read_uint(p, "emission.payload-length"), 66_000);
    assert_absent(p, "object.service");
    assert_eq!(read_bin(p, "object.job.guid"), job_id.as_guid_bytes());
    assert_eq!(read_int(p, "outcome.errno"), -28);
    for gone in ["action", "limit_bytes", "dropped_total", "message"] {
        assert_absent(p, gone);
    }
    assert_eq!(
        dropped_event_message(&dropped, "No space left on device (os error 28)"),
        format!(
            "event peinit.job.ended for job {job_id} (66000 bytes) was refused by the event ring \
             and dropped: No space left on device (os error 28)"
        ),
    );
    assert_eq!(
        kmes_event_subject(&encoded.payload),
        subject,
        "the subject of any event can be read back from its payload",
    );
}

/// The subject of a refused event is read from wherever peinit's events
/// name a service or a job: the object, or a notification's sender.
#[test]
fn the_subject_of_any_event_is_found_in_its_nested_payload() {
    let job_id = job_id();
    let mut record = ended_job_record(
        vec!["app".to_string()],
        system_token(),
        "/sbin/app".to_string(),
        String::new(),
    );
    record.id = job_id;
    record.job_type = JobType::ServiceMain;
    record.service = Some("app".to_string());
    let ended = encode_job_event(&JobEvent::ended(&record).expect("ended"), TIME).expect("ended");
    assert_eq!(
        kmes_event_subject(&ended.payload),
        KmesEventSubject {
            service: Some("app".to_string()),
            job_guid: Some(job_id.as_guid_bytes()),
        }
    );

    let sender = sender(None);
    let status = encode_notify_field_event(
        &sender,
        &NotifyAppliedField::Status {
            text: "up".to_string(),
        },
    )
    .expect("status");
    assert_eq!(
        kmes_event_subject(&status.payload),
        KmesEventSubject {
            service: Some("app".to_string()),
            job_guid: Some(sender.job_id.as_guid_bytes()),
        }
    );

    assert_eq!(kmes_event_subject(b"\xc1 not msgpack"), KmesEventSubject::default());
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

/// PEI-1082: a `peinit.job.ended` carries at most
/// `MAX_JOB_ENDED_ARGUMENTS_BYTES` of arguments, whole arguments only, and
/// says how many it left out — so an event PID 1 cannot emit is never built
/// in the first place.
#[test]
fn a_job_ended_event_cuts_its_arguments_to_the_budget_and_says_so() {
    let arguments: Vec<String> = (0..10).map(|_| "a".repeat(4096)).collect();
    let event = JobEvent::ended(&ended_job_record(
        arguments,
        system_token(),
        "/bin/true".to_string(),
        "exit 1".to_string(),
    ))
    .expect("job ended event");

    let encoded = encode_job_event(&event, TIME).expect("encoded event");

    // Ten 4 KiB arguments encode to 40,990 bytes (4,099 each); seven of
    // them fit the 32,768-byte budget and the eighth does not.
    let kept = read_str_array(&encoded.payload, "object.job.arguments");
    assert_eq!(kept.len(), 7);
    assert!(read_bool(&encoded.payload, "object.job.arguments-truncated"));
    assert_eq!(read_uint(&encoded.payload, "object.job.arguments-count"), 10);
    assert!(
        encoded.payload.len() < 65_536,
        "{} bytes",
        encoded.payload.len()
    );
}

/// PEI-1082, the defaults: a record that fills a default-sized jobs message
/// produces a `peinit.job.ended` that fits a default-sized KMES event with
/// room to spare, however large the record's other fields are — the
/// arguments are never cut, and the whole event stays under the ring's
/// default limit. Both defaults live in this crate and in the kernel
/// respectively: `DEFAULT_MAX_JOBS_MESSAGE_BYTES` (`jobs::socket`, 32768) and
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
    // 128 groups of the longest SID there is, a PATH_MAX image path and a
    // 4 KiB failure cause. The argument is the message less the smallest
    // record that can carry it: a MessagePack string costs at most three
    // bytes over its length, a JSON one at least two plus its framing, so
    // the encoded arguments of any record are smaller than the record.
    const SMALLEST_SUBMIT_RECORD: usize =
        r#"{"command":"submit","image_path":"/","arguments":[""]}"#.len();
    let longest_sid = |index: u32| {
        format!(
            "S-1-5-21-{}-{index}",
            (0..13).map(|_| "4294967295").collect::<Vec<_>>().join("-")
        )
    };
    let mut token_summary = system_token();
    token_summary.user_sid = longest_sid(0);
    token_summary.group_sids = (0..128).map(longest_sid).collect();
    let event = JobEvent::ended(&ended_job_record(
        vec!["a".repeat(DEFAULT_MAX_JOBS_MESSAGE_BYTES - SMALLEST_SUBMIT_RECORD)],
        token_summary,
        "/".to_string() + &"p".repeat(4095),
        "f".repeat(4096),
    ))
    .expect("job ended event");

    let encoded = encode_job_event(&event, TIME).expect("encoded event");

    assert!(!read_bool(&encoded.payload, "object.job.arguments-truncated"));
    assert_eq!(read_bin_array(&encoded.payload, "object.job.token.groups").len(), 128);
    // Header plus payload is what the ring counts; the header and the
    // event type are well under a kilobyte.
    assert!(
        encoded.payload.len() + 1024 <= KMES_DEFAULT_MAX_EVENT_SIZE,
        "{} bytes",
        encoded.payload.len()
    );
}

/// The emission policy is asked before a payload is built, and only for a
/// type that is not essential (PGSS §6.9).
#[test]
fn the_collector_asks_the_policy_before_building_and_never_for_essential_types() {
    let asked = std::cell::RefCell::new(Vec::new());
    let policy = |event_type: &str, tier: EventTier| {
        asked.borrow_mut().push((event_type.to_string(), tier));
        event_type != "peinit.job.created"
    };
    let mut out = EventCollector::new(&policy, TIME);
    let built = std::cell::Cell::new(0);

    out.push("peinit.job.created", |_| {
        built.set(built.get() + 1);
        encode_output_dropped_event(job_id())
    })
    .expect("switched off");
    out.push("peinit.job.output.dropped", |_| {
        built.set(built.get() + 1);
        encode_output_dropped_event(job_id())
    })
    .expect("switched on");
    out.push("peinit.recovery.entered", |_| {
        built.set(built.get() + 1);
        encode_reload_unconfirmed_event("x").map(|event| KmesEvent {
            event_type: "peinit.recovery.entered".to_string(),
            ..event
        })
    })
    .expect("essential");

    assert_eq!(built.get(), 2, "a switched-off type is never built");
    assert_eq!(
        *asked.borrow(),
        [
            ("peinit.job.created".to_string(), EventTier::Verbose),
            ("peinit.job.output.dropped".to_string(), EventTier::Standard),
        ],
        "the policy is never asked about an essential type"
    );
    assert_eq!(out.events().len(), 2);
}

/// The table of types and tiers in code and the fragment describe the same
/// events, so `peinit.evman` matches what peinit emits.
#[test]
fn every_type_peinit_emits_is_in_the_fragment_at_the_same_tier() {
    let fragment = include_str!("../../peinit.evman");
    let mut declared = Vec::new();
    let mut current: Option<String> = None;
    for line in fragment.lines() {
        if let Some(name) = line.strip_prefix("--- event ") {
            current = Some(name.trim().to_string());
        } else if line.starts_with("--- ") {
            current = None;
        } else if let (Some(name), Some(tier)) = (&current, line.strip_prefix("tier: ")) {
            declared.push((name.clone(), tier.trim().to_string()));
        }
    }
    let label = |tier: EventTier| match tier {
        EventTier::Essential => "essential",
        EventTier::Standard => "standard",
        EventTier::Verbose => "verbose",
        EventTier::Debug => "debug",
    };
    let in_code: Vec<(String, String)> = EVENT_TYPES
        .iter()
        .map(|(name, tier)| (name.to_string(), label(*tier).to_string()))
        .collect();
    let mut sorted_declared = declared.clone();
    sorted_declared.sort();
    let mut sorted_code = in_code.clone();
    sorted_code.sort();
    assert_eq!(sorted_code, sorted_declared);
    assert_eq!(tier_of("peinit.event.dropped"), EventTier::Essential);
    assert!(
        !fragment.contains("PROPOSED"),
        "the fragment describes what is emitted, not what is proposed"
    );
}

// ---- helpers -----------------------------------------------------------

/// The values one event declares for one of its fields, from the fragment.
fn declared_values(fragment: &str, event: &str, field: &str) -> Vec<String> {
    let header = format!("--- event {event}");
    let start = fragment.find(&header).expect("event in fragment");
    let body = &fragment[start..];
    let field_line = body
        .find(&format!("field: {field}"))
        .expect("field in event");
    let values_line = body[field_line..]
        .lines()
        .find_map(|line| line.trim().strip_prefix("values: "))
        .expect("values line");
    values_line.split('|').map(|value| value.trim().to_string()).collect()
}

fn sender(operation_id: Option<OperationId>) -> AuthenticatedNotifySender {
    AuthenticatedNotifySender {
        service: "app".to_string(),
        job_id: job_id(),
        operation_id,
        generation: 3,
        job_created_at_ns: 1_000,
        cgroup_generation: 7,
    }
}

fn reload_outcome() -> crate::control::reload_config::ReloadConfigOutcome {
    crate::control::reload_config::ReloadConfigOutcome {
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
    }
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

/// A job's token, summarised as peinit summarises one it queried.
fn system_token() -> TokenSummary {
    TokenSummary::new(
        "SYSTEM",
        "S-1-5-18",
        vec!["S-1-5-32-544".to_string(), "S-1-1-0".to_string()],
        vec![
            "SeTcbPrivilege".to_string(),
            "SeAuditPrivilege".to_string(),
        ],
        vec!["SeTcbPrivilege".to_string()],
    )
}

/// A control-channel caller, summarised from its user SID alone.
fn caller_token() -> TokenSummary {
    TokenSummary::requested_identity("S-1-5-21-1-2-3-1001")
}

fn sid(sddl: &str) -> Vec<u8> {
    peios::security::Sid::from_str(sddl)
        .expect("a SID")
        .as_bytes()
        .to_vec()
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

/// A reader at the value of the dotted `path`, or `None` if it is absent.
fn seek<'a>(payload: &'a [u8], path: &str) -> Option<Reader<'a>> {
    let mut reader = Reader::new(payload);
    'segments: for segment in path.split('.') {
        let count = reader.read_map().ok()?;
        for _ in 0..count {
            let key = reader.read_str().expect("field key");
            if key == segment {
                continue 'segments;
            }
            reader.skip().expect("skip value");
        }
        return None;
    }
    Some(reader)
}

fn field<'a>(payload: &'a [u8], path: &str) -> Reader<'a> {
    seek(payload, path).unwrap_or_else(|| panic!("missing field {path}"))
}

fn read_str(payload: &[u8], path: &str) -> String {
    field(payload, path).read_str().expect(path).to_string()
}

fn read_uint(payload: &[u8], path: &str) -> u64 {
    field(payload, path).read_uint().expect(path)
}

fn read_int(payload: &[u8], path: &str) -> i64 {
    field(payload, path).read_int().expect(path)
}

fn read_bool(payload: &[u8], path: &str) -> bool {
    field(payload, path).read_bool().expect(path)
}

fn read_bin(payload: &[u8], path: &str) -> Vec<u8> {
    field(payload, path).read_bin().expect(path).to_vec()
}

fn read_str_array(payload: &[u8], path: &str) -> Vec<String> {
    let mut reader = field(payload, path);
    let count = reader.read_array().expect(path);
    (0..count)
        .map(|_| reader.read_str().expect(path).to_string())
        .collect()
}

fn read_bin_array(payload: &[u8], path: &str) -> Vec<Vec<u8>> {
    let mut reader = field(payload, path);
    let count = reader.read_array().expect(path);
    (0..count)
        .map(|_| reader.read_bin().expect(path).to_vec())
        .collect()
}

fn assert_absent(payload: &[u8], path: &str) {
    assert!(seek(payload, path).is_none(), "{path} should be absent");
}

fn top_level_keys(payload: &[u8]) -> Vec<String> {
    let mut reader = Reader::new(payload);
    let count = reader.read_map().expect("payload map");
    (0..count)
        .map(|_| {
            let key = reader.read_str().expect("key").to_string();
            reader.skip().expect("value");
            key
        })
        .collect()
}

/// No value anywhere in the payload is nil: absence is an absent key
/// (PGSS §6.5).
fn assert_no_nil(payload: &[u8]) {
    fn walk(reader: &mut Reader<'_>) {
        match reader.peek() {
            Some(Type::Nil) => panic!("a nil value in the payload"),
            Some(Type::Map) => {
                let count = reader.read_map().expect("map");
                for _ in 0..count {
                    reader.read_str().expect("key");
                    walk(reader);
                }
            }
            Some(Type::Array) => {
                let count = reader.read_array().expect("array");
                for _ in 0..count {
                    walk(reader);
                }
            }
            _ => reader.skip().expect("value"),
        }
    }
    walk(&mut Reader::new(payload));
}
