use crate::boundary::ProcessSignal;
use crate::job::{JobRecord, JobState, ProcessHandle, ServiceMainJobSpec};
use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::operation::store::OperationRequest;
use crate::operation::{OperationSource, OperationState, OperationType};
use crate::security::TokenSummary;
use crate::service::ServiceTable;
use crate::service::runtime::{
    ServiceState, ServiceStoppingTimeoutEvidence, ServiceTransition, TransitionCause,
};
use crate::shutdown::{ShutdownFinalizationState, ShutdownKind};
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::super::{ScriptedClock, TestProcessController, alive_service, settings};
use super::SHUTDOWN_NS;
use super::fixture::{DRAINING_STOP_DEADLINE_NS, shutdown_fixture};

#[test]
fn begin_shutdown_applies_initial_shutdown_classification() {
    let mut supervisor = shutdown_fixture();
    let booting_operation = supervisor
        .operations
        .current_for_service("booting")
        .expect("booting operation")
        .id;
    let mut controller = TestProcessController::default();

    let dispatch = supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    assert_eq!(dispatch.runtime.kind, ShutdownKind::Reboot);
    assert_eq!(dispatch.runtime.initiated_at_ns, SHUTDOWN_NS);
    assert_eq!(
        dispatch.runtime.global_deadline_ns,
        SHUTDOWN_NS + 90_000_000_000
    );
    assert_eq!(dispatch.completed_transitions.len(), 1);
    assert_eq!(
        supervisor.service_status("task").expect("task").state,
        ServiceState::Inactive,
    );
    assert_eq!(dispatch.killed_starting.len(), 1);
    assert_eq!(dispatch.killed_starting[0].service, "booting");
    assert_eq!(
        dispatch.killed_starting[0].cgroup_id,
        "/sys/fs/cgroup/peinit/booting",
    );
    assert_eq!(
        supervisor.service_status("booting").expect("booting").state,
        ServiceState::Failed,
    );
    assert_eq!(
        supervisor
            .operation_status(booting_operation)
            .expect("booting operation")
            .state,
        OperationState::Failed,
    );

    assert_eq!(dispatch.first_wave.len(), 2);
    let app_stop = dispatch
        .first_wave
        .iter()
        .find(|stop| stop.service == "app")
        .expect("app stop");
    assert_eq!(app_stop.signal, Some(ProcessSignal::Sigterm));
    assert_eq!(app_stop.target.as_ref().expect("target").pid, 8000);
    assert_eq!(
        app_stop.deadline.as_ref().expect("deadline").due_at_ns,
        SHUTDOWN_NS + 10_000_000_000,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Stopping,
    );
    let already_stopping = dispatch
        .first_wave
        .iter()
        .find(|stop| stop.service == "draining")
        .expect("draining participant");
    assert!(already_stopping.already_stopping);
    assert!(already_stopping.signal.is_none());
    let retained_deadline = already_stopping
        .deadline
        .as_ref()
        .expect("retained stop deadline");
    assert_eq!(retained_deadline.due_at_ns, DRAINING_STOP_DEADLINE_NS);
    assert_eq!(
        retained_deadline.cgroup_id,
        "/sys/fs/cgroup/peinit/draining",
    );
    assert!(retained_deadline.operation_id.is_some());

    assert_eq!(controller.signals.len(), 1);
    assert_eq!(controller.signals[0].target.service, "app");
    assert_eq!(controller.signals[0].signal, ProcessSignal::Sigterm);
    assert_eq!(
        controller.cgroup_kills,
        vec!["/sys/fs/cgroup/peinit/booting".to_string()],
    );
    assert!(supervisor.shutdown().is_some());
}

#[test]
fn starting_services_block_finalization_until_post_kill_resolution() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    supervisor.services =
        ServiceTable::from_boot_snapshot(vec![alive_service("booting")]).expect("service table");
    supervisor
        .services
        .transition_service(
            "booting",
            ServiceTransition {
                to: ServiceState::Starting,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("starting");
    let mut controller = TestProcessController::default();
    controller.set_cgroup_populated("/sys/fs/cgroup/peinit/booting", true);
    let operation_id = supervisor
        .operation_ids
        .allocate_batch(1, SHUTDOWN_NS - 3)
        .expect("operation id")[0];
    supervisor
        .operations
        .request_operation(OperationRequest {
            id: operation_id,
            operation_type: OperationType::Start,
            service: "booting".to_string(),
            source: OperationSource::Admin,
            caller: None,
            created_at_ns: SHUTDOWN_NS - 3,
        })
        .expect("request start");
    supervisor
        .operations
        .start_operation(operation_id, SHUTDOWN_NS - 2)
        .expect("start operation");
    let job_id = supervisor
        .job_ids
        .allocate_batch(1, SHUTDOWN_NS - 2)
        .expect("job id")[0];
    let definition = supervisor
        .services
        .definition("booting")
        .expect("definition")
        .clone();
    let runtime = supervisor
        .services
        .runtime("booting")
        .expect("booting runtime")
        .clone();
    let job = JobRecord::new_service_main(
        job_id,
        ServiceMainJobSpec {
            service: &definition,
            resolved_identity: "SYSTEM".to_string(),
            token_summary: TokenSummary::requested_identity("SYSTEM"),
            activation_generation: runtime.generation,
            cgroup_generation: runtime.cgroup_generation,
            operation_id,
            created_at_ns: SHUTDOWN_NS - 2,
        },
    );
    supervisor.jobs.create_job(job).expect("create job");
    supervisor
        .jobs
        .start_job(
            job_id,
            ProcessHandle {
                pid: 9000,
                pidfd: 90,
            },
            SHUTDOWN_NS - 1,
        )
        .expect("start job");

    let dispatch = supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    assert!(dispatch.first_wave.is_empty());
    assert_eq!(
        dispatch.runtime.finalization,
        ShutdownFinalizationState::WaitingForServices,
    );
    assert_eq!(dispatch.runtime.post_kill_deadlines.len(), 1);
    let post_kill_due = dispatch.runtime.post_kill_deadlines[0].due_at_ns;

    let timeout = supervisor
        .process_due_shutdown_timeouts(&mut controller, post_kill_due)
        .expect("post-kill timeout")
        .expect("timeout dispatch");

    assert_eq!(timeout.abandoned.len(), 1);
    assert_eq!(timeout.job_events.len(), 1);
    assert_eq!(timeout.job_events[0].service.as_deref(), Some("booting"));
    assert_eq!(timeout.job_events[0].state, JobState::Abandoned);
    assert!(
        supervisor
            .jobs()
            .current_service_main_job("booting")
            .is_none()
    );
    assert_eq!(timeout.abandoned[0].service, "booting");
    assert_eq!(
        supervisor.service_status("booting").expect("booting").state,
        ServiceState::Abandoned,
    );
    assert_eq!(timeout.finalization, ShutdownFinalizationState::Ready);
}

#[test]
fn starting_services_fail_unforked_jobs_without_post_kill_retention() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    supervisor.services =
        ServiceTable::from_boot_snapshot(vec![alive_service("booting")]).expect("service table");
    supervisor
        .services
        .transition_service(
            "booting",
            ServiceTransition {
                to: ServiceState::Starting,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("starting");
    let operation_id = supervisor
        .operation_ids
        .allocate_batch(1, SHUTDOWN_NS - 3)
        .expect("operation id")[0];
    supervisor
        .operations
        .request_operation(OperationRequest {
            id: operation_id,
            operation_type: OperationType::Start,
            service: "booting".to_string(),
            source: OperationSource::Admin,
            caller: None,
            created_at_ns: SHUTDOWN_NS - 3,
        })
        .expect("request start");
    supervisor
        .operations
        .start_operation(operation_id, SHUTDOWN_NS - 2)
        .expect("start operation");
    let job_id = supervisor
        .job_ids
        .allocate_batch(1, SHUTDOWN_NS - 2)
        .expect("job id")[0];
    let definition = supervisor
        .services
        .definition("booting")
        .expect("definition")
        .clone();
    let runtime = supervisor
        .services
        .runtime("booting")
        .expect("booting runtime")
        .clone();
    let job = JobRecord::new_service_main(
        job_id,
        ServiceMainJobSpec {
            service: &definition,
            resolved_identity: "SYSTEM".to_string(),
            token_summary: TokenSummary::requested_identity("SYSTEM"),
            activation_generation: runtime.generation,
            cgroup_generation: runtime.cgroup_generation,
            operation_id,
            created_at_ns: SHUTDOWN_NS - 2,
        },
    );
    supervisor.jobs.create_job(job).expect("create job");
    supervisor.pending_launches.push_back(job_id);
    let mut controller = TestProcessController::default();

    let dispatch = supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    assert!(dispatch.runtime.post_kill_deadlines.is_empty());
    assert_eq!(
        dispatch.runtime.finalization,
        ShutdownFinalizationState::Ready,
    );
    assert_eq!(dispatch.startup_job_events.len(), 1);
    assert_eq!(
        dispatch.startup_job_events[0].service.as_deref(),
        Some("booting"),
    );
    assert_eq!(dispatch.startup_job_events[0].state, JobState::Failed);
    assert_eq!(
        dispatch.startup_job_events[0].failure_cause.as_deref(),
        Some("startup cancelled by shutdown"),
    );
    assert!(supervisor.jobs().get(job_id).is_none());
    assert!(supervisor.pending_launch_jobs().is_empty());
}

#[test]
fn already_stopping_without_retained_timeout_evidence_fails_closed() {
    let mut supervisor = shutdown_fixture();
    let operation_id = supervisor
        .operations
        .current_for_service("draining")
        .expect("draining stop operation")
        .id;
    supervisor.control.remove_stop_timeout(operation_id);
    let mut controller = TestProcessController::default();

    let error = supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect_err("missing retained evidence");

    assert!(matches!(
        error,
        crate::supervisor::SupervisorError::Shutdown(
            crate::shutdown::ShutdownError::MissingStoppingTimeoutEvidence { service }
        ) if service == "draining"
    ));
}

#[test]
fn already_stopping_uses_service_level_timeout_evidence_without_active_operation() {
    let mut supervisor = shutdown_fixture();
    let operation_id = supervisor
        .operations
        .current_for_service("draining")
        .expect("draining stop operation")
        .id;
    supervisor.control.remove_stop_timeout(operation_id);
    supervisor
        .operations
        .complete_operation(operation_id, SHUTDOWN_NS - 1, "inactive")
        .expect("complete retained operation");
    supervisor
        .services
        .record_stopping_timeout(
            "draining",
            ServiceStoppingTimeoutEvidence {
                started_at_ns: SHUTDOWN_NS - 2_000_000_000,
                due_at_ns: DRAINING_STOP_DEADLINE_NS,
                cause: TransitionCause::ExplicitStop,
            },
        )
        .expect("service-level stop evidence");
    let mut controller = TestProcessController::default();

    let dispatch = supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    let already_stopping = dispatch
        .first_wave
        .iter()
        .find(|stop| stop.service == "draining")
        .expect("draining participant");
    let deadline = already_stopping
        .deadline
        .as_ref()
        .expect("service-level retained deadline");
    assert_eq!(deadline.started_at_ns, SHUTDOWN_NS - 2_000_000_000);
    assert_eq!(deadline.due_at_ns, DRAINING_STOP_DEADLINE_NS);
    assert_eq!(deadline.operation_id, None);
    assert!(already_stopping.already_stopping);
    assert!(already_stopping.signal.is_none());
}

#[test]
fn already_stopping_with_stale_service_level_timeout_evidence_fails_closed() {
    let mut supervisor = shutdown_fixture();
    let operation_id = supervisor
        .operations
        .current_for_service("draining")
        .expect("draining stop operation")
        .id;
    supervisor.control.remove_stop_timeout(operation_id);
    supervisor
        .operations
        .complete_operation(operation_id, SHUTDOWN_NS - 1, "inactive")
        .expect("complete retained operation");
    supervisor
        .services
        .record_stopping_timeout(
            "draining",
            ServiceStoppingTimeoutEvidence {
                started_at_ns: SHUTDOWN_NS - 2_000_000_000,
                due_at_ns: DRAINING_STOP_DEADLINE_NS,
                cause: TransitionCause::ShutdownWave,
            },
        )
        .expect("stale service-level stop evidence");
    let mut controller = TestProcessController::default();

    let error = supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect_err("stale retained evidence");

    assert!(matches!(
        error,
        crate::supervisor::SupervisorError::Shutdown(
            crate::shutdown::ShutdownError::MissingStoppingTimeoutEvidence { service }
        ) if service == "draining"
    ));
}

#[test]
fn stopping_notify_prevents_shutdown_wave_sigterm_but_keeps_deadline() {
    let mut supervisor = shutdown_fixture();
    let mut notify_controller = TestProcessController::default();
    supervisor
        .apply_notify_datagram(
            datagram(8000, b"STOPPING=1"),
            SHUTDOWN_NS - 1,
            &mut notify_controller,
        )
        .expect("apply stopping notify");
    let mut controller = TestProcessController::default();

    let dispatch = supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    let app_stop = dispatch
        .first_wave
        .iter()
        .find(|stop| stop.service == "app")
        .expect("app stop");
    assert!(!app_stop.already_stopping);
    assert!(app_stop.target.is_some());
    assert!(app_stop.signal.is_none());
    assert_eq!(
        app_stop.deadline.as_ref().expect("deadline").due_at_ns,
        SHUTDOWN_NS + 10_000_000_000,
    );
    assert!(controller.signals.is_empty());
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Stopping,
    );
}

#[test]
fn lifecycle_commands_are_rejected_after_shutdown_begins() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    let mut clock = ScriptedClock::new([SHUTDOWN_NS + 1]);

    let error = supervisor
        .start_service("db", None, &mut clock)
        .expect_err("shutdown rejects start");

    assert!(matches!(
        error,
        crate::supervisor::SupervisorError::Shutdown(
            crate::shutdown::ShutdownError::AlreadyInProgress {
                kind: ShutdownKind::Poweroff
            }
        )
    ));
}

fn datagram(pid: u32, payload: &[u8]) -> NotifyDatagram {
    NotifyDatagram {
        payload: payload.to_vec(),
        credentials: NotifyCredentials {
            pid,
            uid: 0,
            gid: 0,
        },
        fds: Vec::new(),
    }
}
