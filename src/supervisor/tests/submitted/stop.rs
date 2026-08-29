use crate::boundary::{ChildExitStatus, ProcessSignal};
use crate::job::JobState;
use crate::jobs::connection::JobsPendingWait;
use crate::jobs::wire::JobsErrorCode;
use crate::shutdown::ShutdownKind;
use crate::submitted::{JobAccess, SubmittedJobCause};
use crate::supervisor::{JobsCommandError, SupervisorJobsCommandDispatch};

use super::support::{
    Boundaries, LAUNCH_NS, SUBMIT_NS, SUBMITTER, TestJobSecurity, jobs_peer, message, real_pidfd,
    reap, response_json, running_job, submit, submit_payload, submitted_supervisor,
};

const STOP_NS: u64 = LAUNCH_NS + 2_000_000_000;

fn stop_payload(job_id: crate::ids::JobId, wait: bool) -> String {
    format!(
        r#"{{"command":"stop","job_id":"{}","wait":{wait}}}"#,
        job_id.to_canonical_string()
    )
}

#[test]
fn stop_signals_the_job_and_waits_for_it_to_end() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, STOP_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, real_pidfd());
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(&mut supervisor, &peer, message(&stop_payload(job_id, true)));

    assert_eq!(response.error, None);
    assert!(
        response.response.is_none(),
        "answered when the job is terminal"
    );
    assert_eq!(response.wait, Some(JobsPendingWait::Stop { job_id }));
    let Some(SupervisorJobsCommandDispatch::Stop(stop)) = response.dispatch else {
        panic!("expected a stop dispatch, got {:?}", response.dispatch);
    };
    assert_eq!(stop.cause, SubmittedJobCause::ExplicitStop);
    assert!(stop.signalled);
    assert_eq!(boundaries.controller.signals.len(), 1);
    assert_eq!(
        boundaries.controller.signals[0].signal,
        ProcessSignal::Sigterm
    );
    assert_eq!(
        boundaries.security.checks,
        vec![(JobAccess::STOP, SUBMITTER.as_bytes().to_vec())]
    );

    // The process agreed and exited 0: completed, with the stop on record.
    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Exited { code: 0 },
        STOP_NS + 1,
    );
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.state.state, JobState::Completed);
    assert_eq!(view.cause, Some(SubmittedJobCause::ExplicitStop));
}

#[test]
fn stop_without_wait_answers_with_the_view_at_once() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, STOP_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, real_pidfd());
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&stop_payload(job_id, false)),
    );

    assert_eq!(response.wait, None);
    let json = response_json(&response.response.expect("frame").bytes);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["job"]["state"], "running");
    assert_eq!(json["job"]["id"], job_id.to_canonical_string());
}

#[test]
fn a_second_stop_is_accepted_without_a_second_signal() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, STOP_NS, STOP_NS + 1]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, real_pidfd());
    let peer = jobs_peer(SUBMITTER);
    boundaries.run(
        &mut supervisor,
        &peer,
        message(&stop_payload(job_id, false)),
    );

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&stop_payload(job_id, false)),
    );

    assert_eq!(response.error, None);
    assert_eq!(response.dispatch, None, "nothing changed");
    assert_eq!(boundaries.controller.signals.len(), 1);
}

#[test]
fn stopping_a_job_that_has_not_launched_cancels_it() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, STOP_NS]);
    let peer = jobs_peer(SUBMITTER);
    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload("")),
    );

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&stop_payload(job_id, false)),
    );

    let Some(SupervisorJobsCommandDispatch::Cancelled(failure)) = response.dispatch else {
        panic!("expected cancelled, got {:?}", response.dispatch);
    };
    assert_eq!(failure.cause, SubmittedJobCause::ExplicitStop);
    assert_eq!(failure.job_event.state, JobState::Failed);
    assert!(boundaries.controller.signals.is_empty());
    assert!(
        supervisor.pending_submitted_launch_jobs().is_empty(),
        "dequeued"
    );
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.state.state, JobState::Failed);
    assert_eq!(view.state.pid, None);
}

#[test]
fn a_job_that_said_stopping_is_not_signalled_again() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, STOP_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, real_pidfd());
    supervisor
        .apply_notify_datagram(
            crate::notify::NotifyDatagram {
                payload: b"STOPPING=1".to_vec(),
                credentials: crate::notify::NotifyCredentials {
                    pid: 9000,
                    uid: 0,
                    gid: 0,
                },
                fds: Vec::new(),
            },
            LAUNCH_NS + 1,
            &mut boundaries.controller,
        )
        .expect("stopping");
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&stop_payload(job_id, false)),
    );

    let Some(SupervisorJobsCommandDispatch::Stop(stop)) = response.dispatch else {
        panic!("expected stop, got {:?}", response.dispatch);
    };
    assert!(!stop.signalled);
    assert!(boundaries.controller.signals.is_empty());
    assert!(
        supervisor.next_submitted_job_deadline().is_some(),
        "the kill is still scheduled"
    );
}

#[test]
fn stop_is_refused_without_job_stop() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, STOP_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, real_pidfd());
    boundaries.security = TestJobSecurity::allowing(JobAccess::QUERY);
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&stop_payload(job_id, false)),
    );

    assert!(matches!(
        response.error,
        Some(JobsCommandError::AccessDenied(_))
    ));
    let denial = response.access_denial.expect("recorded denial");
    assert_eq!(denial.job_id, job_id);
    assert_eq!(denial.desired_access, JobAccess::STOP);
    assert_eq!(
        response_json(&response.response.expect("frame").bytes)["code"],
        JobsErrorCode::AccessDenied.as_str()
    );
    assert!(boundaries.controller.signals.is_empty());
}

#[test]
fn unknown_and_malformed_job_ids_are_distinguished() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([STOP_NS, STOP_NS + 1]);
    let peer = jobs_peer(SUBMITTER);
    let unknown = crate::ids::JobIdAllocator::new()
        .allocate_batch(1, STOP_NS)
        .expect("job id")[0];

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&stop_payload(unknown, false)),
    );
    assert_eq!(
        response.error,
        Some(JobsCommandError::UnknownJob { job_id: unknown })
    );
    assert_eq!(
        response_json(&response.response.expect("frame").bytes)["code"],
        JobsErrorCode::UnknownJob.as_str()
    );

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(r#"{"command":"stop","job_id":"not-a-job"}"#),
    );
    assert_eq!(response.error, Some(JobsCommandError::InvalidJobId));
}

#[test]
fn signal_delivers_the_number_to_a_running_job() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, STOP_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, real_pidfd());
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&format!(
            r#"{{"command":"signal","job_id":"{}","signal":10}}"#,
            job_id.to_canonical_string()
        )),
    );

    assert_eq!(response.error, None);
    assert_eq!(
        response.dispatch,
        Some(SupervisorJobsCommandDispatch::Signal { job_id, signal: 10 })
    );
    assert_eq!(boundaries.controller.signals.len(), 1);
    assert_eq!(
        boundaries.controller.signals[0].signal,
        ProcessSignal::Number(10)
    );
    assert_eq!(boundaries.controller.signals[0].target.pid, 9000);
    assert_eq!(
        response_json(&response.response.expect("frame").bytes)["status"],
        "ok"
    );
    assert_eq!(
        boundaries.security.checks,
        vec![(JobAccess::SIGNAL, SUBMITTER.as_bytes().to_vec())]
    );
}

#[test]
fn signal_to_a_job_that_is_not_running_is_invalid_state() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, STOP_NS]);
    let peer = jobs_peer(SUBMITTER);
    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload("")),
    );

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&format!(
            r#"{{"command":"signal","job_id":"{}","signal":10}}"#,
            job_id.to_canonical_string()
        )),
    );

    assert!(matches!(
        response.error,
        Some(JobsCommandError::InvalidState { .. })
    ));
    assert!(boundaries.controller.signals.is_empty());
}

#[test]
fn shutdown_stops_every_live_job_at_once_and_records_the_cause() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, SUBMIT_NS + 1, SUBMIT_NS + 2]);
    let first = running_job(&mut supervisor, &mut boundaries, 9000, real_pidfd());
    let second = running_job(&mut supervisor, &mut boundaries, 9001, real_pidfd());
    let peer = jobs_peer(SUBMITTER);
    let queued = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload("")),
    );

    let dispatch = supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut boundaries.controller, STOP_NS)
        .expect("shutdown");

    let mut stopped: Vec<_> = dispatch
        .submitted_stops
        .iter()
        .map(|stop| stop.job_id)
        .collect();
    stopped.sort();
    let mut expected = vec![first, second];
    expected.sort();
    assert_eq!(stopped, expected);
    assert!(
        dispatch
            .submitted_stops
            .iter()
            .all(|stop| stop.cause == SubmittedJobCause::Shutdown)
    );
    let mut signalled: Vec<u32> = boundaries
        .controller
        .signals
        .iter()
        .map(|s| s.target.pid)
        .collect();
    signalled.sort();
    assert_eq!(signalled, vec![9000, 9001]);
    // The queued job never launches: it is cancelled with the same cause.
    let view = supervisor.submitted_job_view(queued).expect("view");
    assert_eq!(view.state.state, JobState::Failed);
    assert_eq!(view.cause, Some(SubmittedJobCause::Shutdown));
    assert!(supervisor.pending_submitted_launch_jobs().is_empty());

    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Exited { code: 0 },
        STOP_NS + 1,
    );
    let view = supervisor.submitted_job_view(first).expect("view");
    assert_eq!(view.cause, Some(SubmittedJobCause::Shutdown));
    assert!(supervisor.shutdown().is_some());
}
