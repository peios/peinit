use crate::execution::launch::{LaunchCreatedJobResult, launch_created_service_main_job};
use crate::job::{JobEventDetail, JobState};
use crate::logging::DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES;

use super::{
    FakeProcessLauncher, FakeTokenProvider, LAUNCHED_AT_NS, SETUP_TIMEOUT_SECS, TEST_NOTIFY_SOCKET,
    ids, job_store_with_service_main, launch_request,
};

#[test]
fn launch_materializes_token_launches_process_and_marks_job_running() {
    let (job_id, operation_id) = ids();
    let mut jobs = job_store_with_service_main(job_id, operation_id);
    let mut tokens = FakeTokenProvider::success();
    let mut launcher = FakeProcessLauncher::success();

    let dispatch = launch_created_service_main_job(
        &mut jobs,
        &mut tokens,
        &mut launcher,
        launch_request(job_id),
    )
    .expect("launch job");
    let LaunchCreatedJobResult::Started(dispatch) = dispatch else {
        panic!("expected started launch");
    };

    assert_eq!(jobs.get(job_id).expect("job").state, JobState::Running);
    assert_eq!(jobs.get(job_id).expect("job").pid, Some(1234));
    assert_eq!(jobs.get(job_id).expect("job").pidfd, Some(9));
    assert_eq!(
        jobs.get(job_id).expect("job").token_summary.user_sid,
        "S-1-5-18"
    );
    assert_eq!(dispatch.job_event.token_summary.user_sid, "S-1-5-18");
    assert_eq!(dispatch.process.pid, 1234);
    assert_eq!(
        dispatch.job_event.detail,
        JobEventDetail::Started {
            started_at_ns: LAUNCHED_AT_NS,
            pid: 1234,
            cgroup_id: "/sys/fs/cgroup/peinit/app/main".to_string(),
        }
    );
    assert_eq!(tokens.observed_jobs, vec![job_id]);
    assert_eq!(launcher.observed.len(), 1);
    assert_eq!(launcher.observed[0].job_id, job_id);
    assert_eq!(launcher.observed[0].token_fd, 8);
    assert_eq!(
        launcher.observed[0].notify_socket.as_deref(),
        Some(TEST_NOTIFY_SOCKET),
    );
    assert_eq!(launcher.observed[0].path.as_deref(), Some("/service/bin"));
    assert_eq!(launcher.observed[0].app_mode.as_deref(), Some("prod"));
    assert_eq!(launcher.observed[0].working_directory, "/srv/app");
    assert_eq!(launcher.observed[0].limit_nofile, Some(4096));
    assert_eq!(launcher.observed[0].limit_core, Some(0));
    assert_eq!(launcher.observed[0].oom_score_adj, -1000);
    assert_eq!(launcher.observed[0].setup_timeout_secs, SETUP_TIMEOUT_SECS);
    assert_eq!(
        launcher.observed[0].output_pipe_buffer_bytes,
        DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES,
    );
}
