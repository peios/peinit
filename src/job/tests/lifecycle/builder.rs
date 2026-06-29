use crate::boot::BootMode;
use crate::boot::phase2::prepare_phase2_boot_plan;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::job::{JobState, ServiceMainJobBuildError, service_main_job_from_phase2_start};
use crate::service::ServiceDefinition;

use super::super::{OBSERVED_AT_NS, service, token_summary};

#[test]
fn phase2_start_builds_created_service_main_job() {
    let service = service();
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let plan = prepare_phase2_boot_plan(
        BootMode::Full,
        std::slice::from_ref(&service),
        10,
        OBSERVED_AT_NS,
        &mut operation_ids,
        &mut job_ids,
    )
    .expect("boot plan");

    let job = service_main_job_from_phase2_start(
        &plan.starts[0],
        &service,
        token_summary("LocalService"),
        3,
        0,
        OBSERVED_AT_NS,
    )
    .expect("service-main job");

    assert_eq!(job.id, plan.starts[0].job_id);
    assert_eq!(job.operation_id, Some(plan.starts[0].operation_id));
    assert_eq!(job.service.as_deref(), Some("app"));
    assert_eq!(job.state, JobState::Created);
    assert_eq!(job.resolved_identity, "LocalService");
    assert_eq!(job.image_path, "/sbin/app");
    assert_eq!(job.arguments, vec!["--foreground", "--ready"]);
    assert_eq!(job.activation_generation, 3);
    assert_eq!(job.cgroup_generation, 0);
    assert_eq!(job.cgroup_id, "/sys/fs/cgroup/peinit/app/main");
}

#[test]
fn phase2_job_build_rejects_mismatched_definition() {
    let app = service();
    let mut other = ServiceDefinition::simple_system_boot("other", "/sbin/other");
    other.identity = "LocalService".to_string();
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let plan = prepare_phase2_boot_plan(
        BootMode::Full,
        &[app],
        10,
        OBSERVED_AT_NS,
        &mut operation_ids,
        &mut job_ids,
    )
    .expect("boot plan");

    let err = service_main_job_from_phase2_start(
        &plan.starts[0],
        &other,
        token_summary("LocalService"),
        0,
        0,
        OBSERVED_AT_NS,
    )
    .expect_err("mismatch");

    assert_eq!(
        err,
        ServiceMainJobBuildError::ServiceMismatch {
            start_service: "app".to_string(),
            definition_service: "other".to_string(),
        }
    );
}
