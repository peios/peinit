use crate::control::query::{QueryError, list_services, service_status};
use crate::job::{JobStore, JobType};
use crate::operation::OperationType;
use crate::operation::store::OperationStore;
use crate::service::runtime::{LeakedCgroupKind, ServiceState, TransitionCause};

use super::{
    create_started_job, job_ids, operation_ids, operation_request, service, service_table,
    transition_to,
};

#[test]
fn service_status_projects_runtime_current_operation_and_current_job() {
    let operation_ids = operation_ids(1);
    let job_ids = job_ids(1);
    let mut services = service_table(&["svc"]);
    transition_to(&mut services, "svc", ServiceState::Active);
    let mut operations = OperationStore::new();
    operations
        .request_operation(operation_request(
            operation_ids[0],
            OperationType::Start,
            "svc",
            1_000,
        ))
        .expect("request operation");
    operations
        .start_operation(operation_ids[0], 1_005)
        .expect("start operation");
    let mut jobs = JobStore::new();
    let mut definition = service("svc", "/sbin/svc");
    definition.identity = "LocalService".to_string();
    create_started_job(&mut jobs, job_ids[0], operation_ids[0], &definition);

    let status = service_status(&services, &operations, &jobs, "svc").expect("status");

    assert_eq!(status.service, "svc");
    assert_eq!(status.state, ServiceState::Active);
    assert_eq!(status.cause, Some(TransitionCause::ExplicitStart));
    assert_eq!(status.generation, 1);
    assert!(!status.definition_removed);
    assert!(status.warnings.is_empty());
    assert!(status.lifecycle_warnings.is_empty());
    let current_operation = status.current_operation.expect("current operation");
    assert_eq!(current_operation.id, operation_ids[0]);
    assert_eq!(current_operation.operation_type, OperationType::Start);
    let current_job = status.current_job.expect("current job");
    assert_eq!(current_job.id, job_ids[0]);
    assert_eq!(current_job.job_type, JobType::ServiceMain);
    assert_eq!(current_job.pid, Some(4242));
    assert_eq!(current_job.started_at_ns, Some(1_010));
    assert_eq!(current_job.identity, "LocalService");
}

#[test]
fn service_status_reports_unknown_service() {
    let services = service_table(&["svc"]);
    let operations = OperationStore::new();
    let jobs = JobStore::new();

    let err = service_status(&services, &operations, &jobs, "missing").expect_err("unknown");

    assert_eq!(
        err,
        QueryError::UnknownService {
            service: "missing".to_string(),
        }
    );
}

#[test]
fn service_status_projects_leaked_cgroup_warnings() {
    let services = service_table_with_leaks();
    let operations = OperationStore::new();
    let jobs = JobStore::new();

    let status = service_status(&services, &operations, &jobs, "svc").expect("status");

    assert_eq!(status.warnings.len(), 2);
    assert_eq!(
        status.warnings[0].path,
        "/sys/fs/cgroup/peinit/svc.gen1/hooks"
    );
    assert_eq!(
        status.warnings[0].warning_type,
        crate::control::query::ServiceStatusWarningType::Hooks,
    );
    assert_eq!(status.warnings[0].detected_at_ns, 1_500);
    assert_eq!(
        status.warnings[1].path,
        "/sys/fs/cgroup/peinit/svc.gen1/health"
    );
    assert_eq!(
        status.lifecycle_warnings,
        vec![
            "service has leaked sub-cgroups from a previous generation -- indicates underlying I/O problem requiring investigation"
                .to_string()
        ],
    );
}

#[test]
fn list_services_projects_sorted_runtime_summaries() {
    let mut services = service_table(&["zeta", "alpha"]);
    transition_to(&mut services, "zeta", ServiceState::Active);
    transition_to(&mut services, "alpha", ServiceState::Active);
    services
        .apply_definition_snapshot(vec![service("alpha", "/sbin/alpha")])
        .expect("remove zeta");

    let list = list_services(&services);

    assert_eq!(list.len(), 2);
    assert_eq!(list[0].service, "alpha");
    assert_eq!(list[0].state, ServiceState::Active);
    assert_eq!(list[0].cause, Some(TransitionCause::ExplicitStart));
    assert!(!list[0].definition_removed);
    assert_eq!(list[1].service, "zeta");
    assert_eq!(list[1].state, ServiceState::Active);
    assert_eq!(list[1].cause, Some(TransitionCause::ExplicitStart));
    assert!(list[1].definition_removed);
}

fn service_table_with_leaks() -> crate::service::ServiceTable {
    let mut services = service_table(&["svc"]);
    services
        .record_leaked_cgroup(
            "svc",
            "/sys/fs/cgroup/peinit/svc.gen1/hooks".to_string(),
            LeakedCgroupKind::Hooks,
            1_500,
        )
        .expect("record hooks leak");
    services
        .record_leaked_cgroup(
            "svc",
            "/sys/fs/cgroup/peinit/svc.gen1/health".to_string(),
            LeakedCgroupKind::Health,
            1_600,
        )
        .expect("record health leak");
    services
}
