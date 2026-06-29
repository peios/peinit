use crate::execution::control::StopTimeoutDeadline;
use crate::job::{JobRecord, ProcessHandle, ServiceMainJobSpec};
use crate::operation::store::OperationRequest;
use crate::operation::{OperationSource, OperationType};
use crate::security::TokenSummary;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceDefinition, ServiceTable};
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::super::{BOOT_NS, TestProcessController, alive_service, settings};
use super::SHUTDOWN_NS;

pub(super) const DRAINING_STOP_DEADLINE_NS: u64 = SHUTDOWN_NS + 4_000_000_000;

pub(super) fn shutdown_fixture() -> Supervisor {
    let db = alive_service("db");
    let mut app = alive_service("app");
    app.requires.push("db".to_string());
    let draining = alive_service("draining");
    let booting = alive_service("booting");
    let mut task = alive_service("task");
    task.service_type = crate::service::ServiceType::Oneshot;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    supervisor.services = ServiceTable::from_boot_snapshot(vec![db, app, draining, booting, task])
        .expect("service table");

    active_service(&mut supervisor, "db", 7000, 40);
    active_service(&mut supervisor, "app", 8000, 50);
    active_service(&mut supervisor, "draining", 8100, 51);
    supervisor
        .services
        .transition_service(
            "draining",
            ServiceTransition {
                to: ServiceState::Stopping,
                cause: TransitionCause::ExplicitStop,
            },
        )
        .expect("draining");
    retain_stop_timeout_evidence(&mut supervisor, "draining", DRAINING_STOP_DEADLINE_NS);
    starting_service(&mut supervisor, "booting");
    completed_service(&mut supervisor, "task");
    supervisor
}

pub(super) fn job_for(supervisor: &Supervisor, service: &str) -> crate::ids::JobId {
    supervisor
        .jobs
        .current_service_main_job(service)
        .unwrap_or_else(|| panic!("{service} job"))
}

pub(super) fn drive_shutdown_to_ready(
    supervisor: &mut Supervisor,
    kind: crate::shutdown::ShutdownKind,
    controller: &mut TestProcessController,
) {
    let app_job = job_for(supervisor, "app");
    let draining_job = job_for(supervisor, "draining");
    let db_job = job_for(supervisor, "db");
    supervisor
        .begin_shutdown(kind, controller, SHUTDOWN_NS)
        .expect("begin shutdown");
    supervisor
        .complete_shutdown_job(app_job, SHUTDOWN_NS + 1, 0, controller)
        .expect("complete app");
    supervisor
        .complete_shutdown_job(draining_job, SHUTDOWN_NS + 2, 0, controller)
        .expect("complete draining");
    supervisor
        .complete_shutdown_job(db_job, SHUTDOWN_NS + 3, 0, controller)
        .expect("complete db");
}

fn active_service(supervisor: &mut Supervisor, service: &str, pid: u32, pidfd: i32) {
    start_runtime(supervisor, service);
    let operation_id = operation_id(supervisor);
    let job_id = job_id(supervisor);
    let definition = supervisor
        .services
        .definition(service)
        .expect("definition")
        .clone();
    let job = service_job(job_id, operation_id, &definition);
    supervisor.jobs.create_job(job).expect("create job");
    supervisor
        .jobs
        .start_job(job_id, ProcessHandle { pid, pidfd }, BOOT_NS + 1)
        .expect("start job");
    supervisor
        .services
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Active,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("active");
}

fn starting_service(supervisor: &mut Supervisor, service: &str) {
    start_runtime(supervisor, service);
    let operation_id = operation_id(supervisor);
    supervisor
        .operations
        .request_operation(OperationRequest {
            id: operation_id,
            operation_type: OperationType::Start,
            service: service.to_string(),
            source: OperationSource::Admin,
            caller: None,
            created_at_ns: BOOT_NS,
        })
        .expect("request start");
    supervisor
        .operations
        .start_operation(operation_id, BOOT_NS + 1)
        .expect("start operation");
}

fn completed_service(supervisor: &mut Supervisor, service: &str) {
    start_runtime(supervisor, service);
    supervisor
        .services
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Completed,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("completed");
}

fn retain_stop_timeout_evidence(supervisor: &mut Supervisor, service: &str, due_at_ns: u64) {
    let operation_id = operation_id(supervisor);
    supervisor
        .operations
        .request_operation(OperationRequest {
            id: operation_id,
            operation_type: OperationType::Stop,
            service: service.to_string(),
            source: OperationSource::Admin,
            caller: None,
            created_at_ns: BOOT_NS,
        })
        .expect("request stop");
    supervisor
        .operations
        .start_operation(operation_id, BOOT_NS + 2)
        .expect("start stop");
    supervisor.control.record_stop_timeout(StopTimeoutDeadline {
        operation_id,
        service: service.to_string(),
        cgroup_id: format!("/sys/fs/cgroup/peinit/{service}/main"),
        due_at_ns,
    });
}

fn start_runtime(supervisor: &mut Supervisor, service: &str) {
    supervisor
        .services
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Starting,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("starting");
}

fn service_job(
    job_id: crate::ids::JobId,
    operation_id: crate::ids::OperationId,
    definition: &ServiceDefinition,
) -> JobRecord {
    JobRecord::new_service_main(
        job_id,
        ServiceMainJobSpec {
            service: definition,
            resolved_identity: "SYSTEM".to_string(),
            token_summary: TokenSummary::requested_identity("SYSTEM"),
            activation_generation: 1,
            cgroup_generation: 0,
            operation_id,
            created_at_ns: BOOT_NS,
        },
    )
}

fn operation_id(supervisor: &mut Supervisor) -> crate::ids::OperationId {
    supervisor
        .operation_ids
        .allocate_batch(1, BOOT_NS)
        .expect("operation id")[0]
}

fn job_id(supervisor: &mut Supervisor) -> crate::ids::JobId {
    supervisor
        .job_ids
        .allocate_batch(1, BOOT_NS)
        .expect("job id")[0]
}
