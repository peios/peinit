//! Which answers carry the process handle: only the submit answer does
//! (TRM §10.7). Every other view of a running job is the bare record.

use crate::control::wire::ControlResponseTimeProjection;
use crate::job::JobState;

use super::support::{
    Boundaries, LAUNCH_NS, SUBMIT_NS, SUBMITTER, jobs_peer, message, real_pidfd, response_json,
    running_job, submitted_supervisor,
};

const VIEW_NS: u64 = LAUNCH_NS + 1_000_000_000;

#[test]
fn only_the_submit_view_carries_the_process_handle() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let pidfd = real_pidfd();
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, pidfd);
    assert_eq!(
        supervisor.jobs.get(job_id).expect("job").state,
        JobState::Running
    );
    let time = ControlResponseTimeProjection::new(VIEW_NS, 0);

    let submit_answer = supervisor
        .jobs_view_frame_with_handle(job_id, time)
        .expect("submit view");
    let fd = submit_answer
        .fd
        .expect("the submit answer carries the handle");
    assert_ne!(
        std::os::fd::AsRawFd::as_raw_fd(&fd),
        pidfd,
        "a duplicate, not peinit's own handle"
    );

    let view = supervisor.jobs_view_frame(job_id, time).expect("view");
    assert!(view.fd.is_none(), "a plain view attaches nothing");
    assert_eq!(view.bytes, submit_answer.bytes, "the record is the same");
    assert_eq!(response_json(&view.bytes)["job"]["state"], "running");
}

#[test]
fn status_of_a_running_job_carries_no_process_handle() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, VIEW_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, real_pidfd());
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&format!(
            r#"{{"command":"status","job_id":"{}"}}"#,
            job_id.to_canonical_string()
        )),
    );

    assert_eq!(response.error, None);
    let frame = response.response.expect("frame");
    assert_eq!(response_json(&frame.bytes)["job"]["state"], "running");
    assert!(
        frame.fd.is_none(),
        "status needs only JOB_QUERY and hands out no handle"
    );
}

#[test]
fn stop_and_signal_answers_carry_no_process_handle() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, VIEW_NS, VIEW_NS + 1]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, real_pidfd());
    let peer = jobs_peer(SUBMITTER);

    let signalled = boundaries.run(
        &mut supervisor,
        &peer,
        message(&format!(
            r#"{{"command":"signal","job_id":"{}","signal":10}}"#,
            job_id.to_canonical_string()
        )),
    );
    assert_eq!(signalled.error, None);
    let frame = signalled.response.expect("signal frame");
    assert_eq!(response_json(&frame.bytes)["job"]["state"], "running");
    assert!(frame.fd.is_none());

    let stopped = boundaries.run(
        &mut supervisor,
        &peer,
        message(&format!(
            r#"{{"command":"stop","job_id":"{}","wait":false}}"#,
            job_id.to_canonical_string()
        )),
    );
    assert_eq!(stopped.error, None);
    let frame = stopped.response.expect("stop frame");
    assert_eq!(response_json(&frame.bytes)["job"]["state"], "running");
    assert!(frame.fd.is_none());
}
