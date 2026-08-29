use crate::boundary::JobIdentityError;
use crate::job::{JobState, JobType};
use crate::jobs::socket::JobsSocketLimits;
use crate::jobs::wire::JobsErrorCode;
use crate::shutdown::ShutdownKind;
use crate::submitted::JobReadiness;
use crate::supervisor::{JobsCommandError, SupervisorJobsCommandDispatch};

use super::support::{
    Boundaries, IDENTITY_LOGON_SESSION, IDENTITY_SID, SUBMIT_NS, SUBMITTER, SYSTEM_SID, jobs_peer,
    message, message_with, response_json, set_jobs_limits, submit, submit_payload,
    submitted_supervisor,
};

#[test]
fn submit_creates_a_created_record_and_queues_the_launch() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&submit_payload(
            r#""description":"nightly","arguments":["--full"]"#,
        )),
    );

    assert_eq!(response.error, None);
    assert!(
        response.response.is_none(),
        "submit answers when the job leaves created"
    );
    let Some(SupervisorJobsCommandDispatch::Submit(dispatch)) = response.dispatch else {
        panic!("expected submit dispatch");
    };
    let job_id = dispatch.job_event.job_id;
    assert_eq!(dispatch.submitter_sid, SUBMITTER);
    assert_eq!(dispatch.job_event.job_type, JobType::Submitted);
    assert_eq!(dispatch.job_event.state, JobState::Created);
    assert_eq!(dispatch.job_event.image_path, "/usr/bin/backup");
    assert_eq!(dispatch.job_event.arguments, vec!["--full".to_string()]);
    assert_eq!(dispatch.job_event.resolved_identity, IDENTITY_SID);
    assert_eq!(
        response.wait,
        Some(crate::jobs::connection::JobsPendingWait::Submit { job_id })
    );

    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.state.state, JobState::Created);
    assert_eq!(view.submitter_sid, SUBMITTER);
    assert_eq!(view.identity_sid, IDENTITY_SID);
    assert_eq!(view.logon_session, IDENTITY_LOGON_SESSION);
    assert_eq!(view.description, "nightly");
    assert_eq!(view.created_at_ns, SUBMIT_NS);
    assert_eq!(view.ready, None, "readiness none has no ready flag");
    assert_eq!(supervisor.pending_submitted_launch_jobs(), vec![job_id]);
    assert_eq!(boundaries.identity.peer_primary_sources, 1);
    assert_eq!(boundaries.identity.attached_token_sources, 0);
    let entry = supervisor.submitted_jobs().get(job_id).expect("entry");
    assert_eq!(entry.security_descriptor.bytes, SUBMITTER.as_bytes());
    assert!(entry.prepared_token_fd.is_some());
}

#[test]
fn an_attached_token_is_the_job_identity() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);

    submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message_with(&submit_payload(""), true, 0),
    );

    assert_eq!(boundaries.identity.attached_token_sources, 1);
    assert_eq!(boundaries.identity.peer_primary_sources, 0);
}

#[test]
fn a_rejected_token_refuses_the_submission_and_leaves_nothing_behind() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    boundaries.identity.refuse_with = Some(JobIdentityError::BadToken(
        "impersonation level below Impersonation".to_string(),
    ));
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message_with(&submit_payload(""), true, 0),
    );

    assert!(matches!(
        response.error,
        Some(JobsCommandError::BadToken(_))
    ));
    let frame = response.response.expect("error frame");
    let json = response_json(&frame.bytes);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], JobsErrorCode::BadToken.as_str());
    assert!(supervisor.pending_submitted_launch_jobs().is_empty());
    assert!(supervisor.submitted_jobs().ids().is_empty());
}

#[test]
fn a_malformed_definition_is_invalid_arguments() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(r#"{"command":"submit","image_path":"relative/path"}"#),
    );

    assert!(matches!(
        response.error,
        Some(JobsCommandError::Definition(_))
    ));
    let json = response_json(&response.response.expect("frame").bytes);
    assert_eq!(json["code"], JobsErrorCode::InvalidArguments.as_str());
    assert_eq!(
        boundaries.identity.peer_primary_sources, 0,
        "refused before identity"
    );
    assert!(supervisor.submitted_jobs().ids().is_empty());
}

#[test]
fn descriptor_count_must_match_the_attachments() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message_with(
            &submit_payload(r#""descriptors":["control"],"output":true"#),
            false,
            1,
        ),
    );

    assert!(matches!(
        response.error,
        Some(JobsCommandError::Definition(
            crate::submitted::SubmittedJobDefinitionError::DescriptorCountMismatch {
                expected: 2,
                attached: 1,
            }
        ))
    ));
}

#[test]
fn named_descriptors_and_the_output_sink_are_kept_on_the_entry() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);

    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message_with(
            &submit_payload(
                r#""descriptors":["control","data"],"output":true,"readiness":"notify""#,
            ),
            false,
            3,
        ),
    );

    let entry = supervisor.submitted_jobs().get(job_id).expect("entry");
    let names: Vec<&str> = entry
        .attached_descriptors
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(names, vec!["control", "data"]);
    assert!(entry.output_sink_fd.is_some());
    assert_eq!(entry.definition.readiness, JobReadiness::Notify);
    assert_eq!(entry.ready, Some(false));
    assert_eq!(
        supervisor.submitted_job_view(job_id).expect("view").ready,
        Some(false)
    );
}

#[test]
fn a_submitter_supplied_descriptor_is_used_as_given() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);

    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload(
            r#""security_descriptor":"O:BAG:BAD:(A;;0x7;;;BA)""#,
        )),
    );

    assert_eq!(
        supervisor
            .submitted_jobs()
            .get(job_id)
            .expect("entry")
            .security_descriptor
            .bytes,
        b"sddl:O:BAG:BAD:(A;;0x7;;;BA)"
    );
}

#[test]
fn an_invalid_supplied_descriptor_is_refused() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    boundaries.security.sddl_error = Some("no owner".to_string());
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(
        &mut supervisor,
        &peer,
        message(&submit_payload(r#""security_descriptor":"D:(A;;GA;;;BA)""#)),
    );

    assert_eq!(
        response.error,
        Some(JobsCommandError::InvalidDescriptor("no owner".to_string()))
    );
    assert!(supervisor.submitted_jobs().ids().is_empty());
}

#[test]
fn the_quota_counts_live_jobs_per_submitter_and_exempts_system() {
    let mut supervisor = submitted_supervisor();
    set_jobs_limits(
        &mut supervisor,
        JobsSocketLimits {
            max_jobs_per_submitter: 1,
            ..JobsSocketLimits::default()
        },
    );
    let mut boundaries = Boundaries::at([SUBMIT_NS, SUBMIT_NS + 1, SUBMIT_NS + 2, SUBMIT_NS + 3]);
    let backupd = jobs_peer(SUBMITTER);
    let other = jobs_peer("atriumd");
    let system = jobs_peer(SYSTEM_SID);

    submit(
        &mut supervisor,
        &mut boundaries,
        &backupd,
        message(&submit_payload("")),
    );
    let refused = boundaries.run(&mut supervisor, &backupd, message(&submit_payload("")));
    assert_eq!(
        refused.error,
        Some(JobsCommandError::QuotaExceeded {
            submitter_sid: SUBMITTER.to_string(),
            live: 1,
            limit: 1,
        })
    );
    assert_eq!(
        response_json(&refused.response.expect("frame").bytes)["code"],
        JobsErrorCode::QuotaExceeded.as_str()
    );
    // Another submitter has its own count; SYSTEM has none.
    submit(
        &mut supervisor,
        &mut boundaries,
        &other,
        message(&submit_payload("")),
    );
    submit(
        &mut supervisor,
        &mut boundaries,
        &system,
        message(&submit_payload("")),
    );
    assert_eq!(supervisor.submitted_jobs().ids().len(), 3);
}

#[test]
fn submissions_are_refused_during_shutdown() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    supervisor
        .begin_shutdown(
            ShutdownKind::Poweroff,
            &mut boundaries.controller,
            SUBMIT_NS - 1,
        )
        .expect("shutdown");
    let peer = jobs_peer(SUBMITTER);

    let response = boundaries.run(&mut supervisor, &peer, message(&submit_payload("")));

    assert_eq!(response.error, Some(JobsCommandError::ShuttingDown));
    assert_eq!(
        response_json(&response.response.expect("frame").bytes)["code"],
        JobsErrorCode::InvalidState.as_str()
    );
}

#[test]
fn a_truncated_message_is_refused_and_closes_the_connection() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);
    let mut truncated = message(&submit_payload(""));
    truncated.truncated = true;

    let response = boundaries.run(&mut supervisor, &peer, truncated);

    assert_eq!(response.error, Some(JobsCommandError::Truncated));
    assert!(response.close_after);
}
