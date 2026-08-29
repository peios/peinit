use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::submitted::{JobProgress, JobProgressUnit, SubmittedNotifyField};
use crate::supervisor::{
    SupervisorError, SupervisorNotifyOutcome, SupervisorSubmittedNotifyDispatch,
};

use super::support::{
    Boundaries, LAUNCH_NS, SUBMIT_NS, SUBMITTER, jobs_peer, launch, message, running_job, submit,
    submit_payload, submitted_supervisor,
};
use crate::supervisor::JOB_STATUS_EVENT_INTERVAL_NS;

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

fn notify(
    supervisor: &mut crate::supervisor::Supervisor,
    boundaries: &mut Boundaries,
    pid: u32,
    payload: &[u8],
    at_ns: u64,
) -> Result<SupervisorSubmittedNotifyDispatch, SupervisorError> {
    match supervisor.apply_notify_datagram(
        datagram(pid, payload),
        at_ns,
        &mut boundaries.controller,
    )? {
        SupervisorNotifyOutcome::SubmittedJob(dispatch) => Ok(dispatch),
        SupervisorNotifyOutcome::Service(dispatch) => {
            panic!("expected a job notify, got {dispatch:?}")
        }
    }
}

#[test]
fn a_running_job_is_authenticated_by_pid_and_pidfd() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);

    let dispatch = notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"STATUS=Scanning /data",
        LAUNCH_NS + 1,
    )
    .expect("status");

    assert_eq!(dispatch.job_id, job_id);
    assert_eq!(dispatch.submitter_sid, SUBMITTER);
    assert_eq!(
        dispatch.applied,
        vec![SubmittedNotifyField::Status {
            text: "Scanning /data".to_string()
        }]
    );
    assert_eq!(dispatch.status_text.as_deref(), Some("Scanning /data"));
    assert!(dispatch.status_event_due, "the first report is always due");
    assert_eq!(boundaries.controller.pidfd_match_checks, vec![(90, 9000)]);
    assert_eq!(
        supervisor
            .submitted_job_view(job_id)
            .expect("view")
            .status_text
            .as_deref(),
        Some("Scanning /data")
    );
}

#[test]
fn a_pidfd_mismatch_rejects_the_notification() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);
    boundaries.controller.set_pidfd_match(90, 9000, false);

    let error = notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"STATUS=x",
        LAUNCH_NS + 1,
    )
    .expect_err("rejected");

    assert!(matches!(
        error,
        SupervisorError::Notify(crate::execution::notify::NotifyApplyError::PidfdMismatch {
            pid: 9000,
            pidfd: 90,
            ..
        })
    ));
    assert_eq!(
        boundaries.controller.pidfd_match_checks,
        vec![(90, 9000)],
        "verified once"
    );
    assert_eq!(
        supervisor
            .submitted_job_view(job_id)
            .expect("view")
            .status_text,
        None
    );
}

#[test]
fn an_unknown_sender_is_unauthenticated() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    running_job(&mut supervisor, &mut boundaries, 9000, 90);

    let error = supervisor
        .apply_notify_datagram(
            datagram(9999, b"STATUS=x"),
            LAUNCH_NS + 1,
            &mut boundaries.controller,
        )
        .expect_err("rejected");

    assert!(matches!(
        error,
        SupervisorError::Notify(
            crate::execution::notify::NotifyApplyError::UnauthenticatedSender { .. }
        )
    ));
}

#[test]
fn progress_forms_are_retained_and_unit_is_separate() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);

    let open = notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"PROGRESS=5\nPROGRESS_UNIT=items",
        LAUNCH_NS + 1,
    )
    .expect("progress");
    assert_eq!(
        open.applied,
        vec![
            SubmittedNotifyField::Progress {
                progress: JobProgress {
                    current: 5,
                    total: None,
                    bounded: false,
                }
            },
            SubmittedNotifyField::ProgressUnit {
                unit: JobProgressUnit::Items
            },
        ]
    );
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.progress_unit, Some(JobProgressUnit::Items));

    notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"PROGRESS=6/",
        LAUNCH_NS + 2,
    )
    .expect("bounded");
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(
        view.progress,
        Some(JobProgress {
            current: 6,
            total: None,
            bounded: true,
        })
    );
    assert_eq!(
        view.progress_unit,
        Some(JobProgressUnit::Items),
        "the unit outlives the report"
    );

    notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"PROGRESS=7/10",
        LAUNCH_NS + 3,
    )
    .expect("total");
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(
        view.progress,
        Some(JobProgress {
            current: 7,
            total: Some(10),
            bounded: true,
        })
    );
}

#[test]
fn a_malformed_progress_is_ignored_and_the_rest_of_the_datagram_applied() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);

    let dispatch = notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"PROGRESS=12/5\nPROGRESS_UNIT=furlongs\nSTATUS=still going",
        LAUNCH_NS + 1,
    )
    .expect("applied");

    // §4.17: an unexpected value drops the line, not the datagram; and
    // nothing is clamped or repaired.
    assert_eq!(
        dispatch.applied,
        vec![SubmittedNotifyField::Status {
            text: "still going".to_string()
        }]
    );
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.progress, None);
    assert_eq!(view.progress_unit, None);
    assert_eq!(view.status_text.as_deref(), Some("still going"));
}

#[test]
fn status_events_are_rate_limited_per_job() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    running_job(&mut supervisor, &mut boundaries, 9000, 90);

    let first = notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"PROGRESS=1",
        LAUNCH_NS + 1,
    )
    .expect("first");
    let soon = notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"PROGRESS=2",
        LAUNCH_NS + 2,
    )
    .expect("second");
    let later = notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"PROGRESS=3",
        LAUNCH_NS + 1 + JOB_STATUS_EVENT_INTERVAL_NS,
    )
    .expect("third");

    assert!(first.status_event_due);
    assert!(!soon.status_event_due, "within a second of the last event");
    assert!(later.status_event_due);
    assert_eq!(
        later.progress,
        Some(JobProgress {
            current: 3,
            total: None,
            bounded: false,
        }),
        "the value is retained even when no event is due"
    );
}

#[test]
fn ready_satisfies_notify_readiness_and_is_idempotent() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);
    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload(r#""readiness":"notify""#)),
    );
    launch(&mut supervisor, &mut boundaries.controller, 9000, 90);
    assert!(
        supervisor.next_submitted_job_deadline().is_some(),
        "readiness timeout armed"
    );

    let ready = notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"READY=1",
        LAUNCH_NS + 1,
    )
    .expect("ready");
    assert_eq!(ready.applied, vec![SubmittedNotifyField::Ready]);
    assert_eq!(
        supervisor.submitted_job_view(job_id).expect("view").ready,
        Some(true)
    );
    assert!(
        supervisor.next_submitted_job_deadline().is_none(),
        "readiness timeout disarmed"
    );

    let again = notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"READY=1",
        LAUNCH_NS + 2,
    )
    .expect("again");
    assert!(again.applied.is_empty(), "a second READY=1 changes nothing");
}

#[test]
fn ready_from_a_job_without_notify_readiness_is_ignored() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);

    let dispatch = notify(
        &mut supervisor,
        &mut boundaries,
        9000,
        b"READY=1",
        LAUNCH_NS + 1,
    )
    .expect("ready");

    assert!(dispatch.applied.is_empty());
    assert_eq!(
        supervisor.submitted_job_view(job_id).expect("view").ready,
        None
    );
}
