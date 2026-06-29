use crate::boundary::BoundaryError;
use crate::execution::launch::{LaunchCreatedJobError, launch_created_service_main_job};
use crate::job::{JobRecord, JobState, JobStore};

use super::{
    CREATED_AT_NS, FakeProcessLauncher, FakeTokenProvider, ids, job_store_with_service_main,
    launch_request, token_summary,
};

#[test]
fn token_failure_does_not_mutate_job_or_launch_process() {
    let (job_id, operation_id) = ids();
    let mut jobs = job_store_with_service_main(job_id, operation_id);
    let mut tokens = FakeTokenProvider::failure(BoundaryError::Token("authd".to_string()));
    let mut launcher = FakeProcessLauncher::success();

    let err = launch_created_service_main_job(
        &mut jobs,
        &mut tokens,
        &mut launcher,
        launch_request(job_id),
    )
    .expect_err("token failure");

    assert_eq!(
        err,
        LaunchCreatedJobError::Boundary(BoundaryError::Token("authd".to_string()))
    );
    assert_eq!(jobs.get(job_id).expect("job").state, JobState::Created);
    assert_eq!(tokens.observed_jobs, vec![job_id]);
    assert!(launcher.observed.is_empty());
}

#[test]
fn process_launch_failure_does_not_mark_job_running() {
    let (job_id, operation_id) = ids();
    let mut jobs = job_store_with_service_main(job_id, operation_id);
    let mut tokens = FakeTokenProvider::success();
    let mut launcher = FakeProcessLauncher::failure(BoundaryError::Process("clone3".to_string()));

    let err = launch_created_service_main_job(
        &mut jobs,
        &mut tokens,
        &mut launcher,
        launch_request(job_id),
    )
    .expect_err("launch failure");

    assert_eq!(
        err,
        LaunchCreatedJobError::Boundary(BoundaryError::Process("clone3".to_string()))
    );
    assert_eq!(jobs.get(job_id).expect("job").state, JobState::Created);
    assert_eq!(tokens.observed_jobs, vec![job_id]);
    assert_eq!(launcher.observed.len(), 1);
    assert_eq!(launcher.observed[0].job_id, job_id);
    assert_eq!(launcher.observed[0].token_fd, 8);
}

#[test]
fn invalid_job_is_rejected_before_boundaries_are_called() {
    let (job_id, _) = ids();
    let mut jobs = JobStore::new();
    jobs.create_job(JobRecord::new_ad_hoc(
        job_id,
        "SYSTEM",
        token_summary(),
        "/bin/true",
        Vec::new(),
        CREATED_AT_NS,
    ))
    .expect("create ad-hoc");
    let mut tokens = FakeTokenProvider::success();
    let mut launcher = FakeProcessLauncher::success();

    let err = launch_created_service_main_job(
        &mut jobs,
        &mut tokens,
        &mut launcher,
        launch_request(job_id),
    )
    .expect_err("not service-main");

    assert_eq!(err, LaunchCreatedJobError::NotServiceMainJob { job_id });
    assert!(tokens.observed_jobs.is_empty());
    assert!(launcher.observed.is_empty());
}
