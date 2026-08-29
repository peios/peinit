use crate::boundary::{ChildExitStatus, ProcessSignal};
use crate::control::system::ControlPeer;
use crate::control::wire::parse_control_request;
use crate::job::JobState;
use crate::security::TokenSummary;
use crate::submitted::{JobAccess, SubmittedJobCause};
use crate::supervisor::control_command::{
    SupervisorControlCommandBodyError, SupervisorControlCommandBodyResponse,
};

use super::super::ScriptedClock;
use super::support::{
    Boundaries, LAUNCH_NS, SUBMIT_NS, SUBMITTER, TestJobSecurity, jobs_peer, message, reap,
    running_job, submit, submit_payload, submitted_supervisor,
};

const CONTROL_NS: u64 = LAUNCH_NS + 3_000_000_000;
/// A second submitter, as a literal SID: the filters take SIDs, not names.
const OTHER_SID: &str = "S-1-5-21-1-2-3-2002";

fn admin() -> ControlPeer {
    ControlPeer::borrowed_token_fd(60, TokenSummary::requested_identity("admin"))
}

fn accepted_line(response: SupervisorControlCommandBodyResponse) -> serde_json::Value {
    match response {
        SupervisorControlCommandBodyResponse::Accepted {
            response_line: Some(line),
            ..
        } => {
            assert_eq!(line.last(), Some(&b'\n'));
            serde_json::from_slice(&line[..line.len() - 1]).expect("json")
        }
        other => panic!("expected an accepted response with a line, got {other:?}"),
    }
}

#[test]
fn job_status_returns_the_view_to_a_caller_with_job_query() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);
    let parsed = parse_control_request(
        format!(r#"{{"command":"job-status","job_id":"{job_id}"}}"#).as_bytes(),
    )
    .expect("parsed");
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    let response = supervisor
        .run_control_job_status(&parsed, &admin(), &mut boundaries.security, &mut clock)
        .expect("status");

    let json = accepted_line(response);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["job"]["id"], job_id.to_canonical_string());
    assert_eq!(json["job"]["type"], "submitted");
    assert_eq!(json["job"]["state"], "running");
    assert_eq!(json["job"]["pid"], 9000);
    assert_eq!(json["job"]["submitter"], SUBMITTER);
    assert_eq!(
        boundaries.security.checks,
        vec![(JobAccess::QUERY, SUBMITTER.as_bytes().to_vec())]
    );
}

#[test]
fn job_status_without_job_query_is_access_denied_not_unknown() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);
    let mut security = TestJobSecurity::allowing(JobAccess::STOP);
    let parsed = parse_control_request(
        format!(r#"{{"command":"job-status","job_id":"{job_id}"}}"#).as_bytes(),
    )
    .expect("parsed");
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    let error = supervisor
        .run_control_job_status(&parsed, &admin(), &mut security, &mut clock)
        .expect_err("denied");

    let SupervisorControlCommandBodyError::JobAccessDenied(denied) = error else {
        panic!("expected a job access denial, got {error:?}");
    };
    assert_eq!(denied.job_id, job_id);
    assert_eq!(denied.desired_access, JobAccess::QUERY);
}

#[test]
fn job_status_for_an_unknown_job_is_unknown_job() {
    let supervisor = submitted_supervisor();
    let mut security = TestJobSecurity::default();
    let unknown = crate::ids::JobIdAllocator::new()
        .allocate_batch(1, CONTROL_NS)
        .expect("job id")[0];
    let parsed = parse_control_request(
        format!(r#"{{"command":"job-status","job_id":"{unknown}"}}"#).as_bytes(),
    )
    .expect("parsed");
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    let error = supervisor
        .run_control_job_status(&parsed, &admin(), &mut security, &mut clock)
        .expect_err("unknown");

    assert!(
        matches!(error, SupervisorControlCommandBodyError::UnknownJob { job_id } if job_id == unknown)
    );
    assert!(security.checks.is_empty(), "nothing to check against");
}

#[test]
fn job_list_filters_by_state_and_hides_jobs_without_job_query() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, SUBMIT_NS + 1, SUBMIT_NS + 2]);
    let running = running_job(&mut supervisor, &mut boundaries, 9000, 90);
    let ended = running_job(&mut supervisor, &mut boundaries, 9001, 91);
    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9001,
        ChildExitStatus::Exited { code: 0 },
        LAUNCH_NS + 1,
    );
    let other = jobs_peer("atriumd");
    let others = submit(
        &mut supervisor,
        &mut boundaries,
        &other,
        message(&submit_payload("")),
    );
    let mut clock = ScriptedClock::new([CONTROL_NS, CONTROL_NS + 1, CONTROL_NS + 2]);

    let all = accepted_line(
        supervisor
            .run_control_job_list(
                &parse_control_request(br#"{"command":"job-list"}"#).expect("parsed"),
                &admin(),
                &mut boundaries.security,
                &mut clock,
            )
            .expect("list"),
    );
    let mut ids: Vec<String> = all["jobs"]
        .as_array()
        .expect("jobs")
        .iter()
        .map(|job| job["id"].as_str().expect("id").to_string())
        .collect();
    ids.sort();
    let mut expected = vec![
        running.to_canonical_string(),
        ended.to_canonical_string(),
        others.to_canonical_string(),
    ];
    expected.sort();
    assert_eq!(ids, expected);

    let running_only = accepted_line(
        supervisor
            .run_control_job_list(
                &parse_control_request(br#"{"command":"job-list","state":"running"}"#)
                    .expect("parsed"),
                &admin(),
                &mut boundaries.security,
                &mut clock,
            )
            .expect("list"),
    );
    assert_eq!(running_only["jobs"].as_array().expect("jobs").len(), 1);
    assert_eq!(running_only["jobs"][0]["id"], running.to_canonical_string());

    // A caller with JOB_QUERY on nothing sees an empty list, not an error.
    let mut none = TestJobSecurity::allowing(JobAccess::STOP);
    let hidden = accepted_line(
        supervisor
            .run_control_job_list(
                &parse_control_request(br#"{"command":"job-list"}"#).expect("parsed"),
                &admin(),
                &mut none,
                &mut clock,
            )
            .expect("list"),
    );
    assert_eq!(hidden["status"], "ok");
    assert!(hidden["jobs"].as_array().expect("jobs").is_empty());
    assert_eq!(none.checks.len(), 3, "every job was checked");
}

#[test]
fn job_list_filters_by_submitter() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, SUBMIT_NS + 1]);
    running_job(&mut supervisor, &mut boundaries, 9000, 90);
    let other = jobs_peer(OTHER_SID);
    let others = submit(
        &mut supervisor,
        &mut boundaries,
        &other,
        message(&submit_payload("")),
    );
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    let listed = accepted_line(
        supervisor
            .run_control_job_list(
                &parse_control_request(
                    format!(r#"{{"command":"job-list","submitter":"{OTHER_SID}"}}"#).as_bytes(),
                )
                .expect("parsed"),
                &admin(),
                &mut boundaries.security,
                &mut clock,
            )
            .expect("list"),
    );

    let jobs = listed["jobs"].as_array().expect("jobs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["id"], others.to_canonical_string());
    assert_eq!(jobs[0]["state"], "created");
}

#[test]
fn job_stop_signals_the_job_and_answers_with_the_view() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);
    let parsed = parse_control_request(
        format!(r#"{{"command":"job-stop","job_id":"{job_id}","wait":false}}"#).as_bytes(),
    )
    .expect("parsed");
    let mut clock = ScriptedClock::new([CONTROL_NS, CONTROL_NS + 1]);

    let response = supervisor
        .run_control_job_stop(
            &parsed,
            &admin(),
            &mut boundaries.security,
            &mut boundaries.controller,
            &mut clock,
        )
        .expect("stop");

    let json = accepted_line(response);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["job"]["state"], "running");
    assert_eq!(boundaries.controller.signals.len(), 1);
    assert_eq!(
        boundaries.controller.signals[0].signal,
        ProcessSignal::Sigterm
    );
    assert_eq!(
        boundaries.security.checks,
        vec![(JobAccess::STOP, SUBMITTER.as_bytes().to_vec())]
    );
    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Signaled {
            signal: libc::SIGTERM,
            core_dumped: false,
        },
        CONTROL_NS + 2,
    );
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.state.state, JobState::Failed);
    assert_eq!(view.state.exit_signal, Some(libc::SIGTERM));
    assert_eq!(view.cause, Some(SubmittedJobCause::ExplicitStop));
}

#[test]
fn job_stop_with_wait_registers_a_job_wait() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);
    let parsed = parse_control_request(
        format!(r#"{{"command":"job-stop","job_id":"{job_id}"}}"#).as_bytes(),
    )
    .expect("parsed");
    let mut clock = ScriptedClock::new([CONTROL_NS, CONTROL_NS + 1]);

    let response = supervisor
        .run_control_job_stop(
            &parsed,
            &admin(),
            &mut boundaries.security,
            &mut boundaries.controller,
            &mut clock,
        )
        .expect("stop");

    let SupervisorControlCommandBodyResponse::Accepted {
        response_line: None,
        wait: Some(crate::control::connection::ControlPendingWait::Job { job_id: waited }),
        ..
    } = response
    else {
        panic!("expected a registered job wait, got {response:?}");
    };
    assert_eq!(waited, job_id);
}
