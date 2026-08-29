use crate::boundary::{ChildExitStatus, ProcessSignal};
use crate::job::JobState;
use crate::submitted::{SubmittedJobCause, SubmittedJobDeadlineKind};
use crate::supervisor::{SupervisorLifecycleDeadlineKind, SupervisorSubmittedDeadlineDispatch};

use super::support::{
    Boundaries, LAUNCH_NS, SUBMIT_NS, SUBMITTER, jobs_peer, launch, message, reap, running_job,
    submit, submit_payload, submitted_supervisor,
};

const SECOND_NS: u64 = 1_000_000_000;

fn deadline_kind(supervisor: &crate::supervisor::Supervisor) -> (SubmittedJobDeadlineKind, u64) {
    let deadline = supervisor
        .next_submitted_job_deadline()
        .expect("a deadline");
    let SupervisorLifecycleDeadlineKind::SubmittedJob { kind, .. } = deadline.kind else {
        panic!("expected a submitted deadline, got {:?}", deadline.kind);
    };
    (kind, deadline.due_at_ns)
}

#[test]
fn a_job_without_timeouts_has_no_deadline() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    running_job(&mut supervisor, &mut boundaries, 9000, 90);

    assert!(supervisor.next_submitted_job_deadline().is_none());
}

#[test]
fn the_timeout_stops_the_job_and_the_stop_timeout_kills_it() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);
    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload(r#""timeout":30,"stop_timeout":5"#)),
    );
    launch(&mut supervisor, &mut boundaries.controller, 9000, 90);
    let cgroup = crate::job::submitted_job_cgroup_path(job_id);

    let (kind, due_at_ns) = deadline_kind(&supervisor);
    assert_eq!(kind, SubmittedJobDeadlineKind::Timeout);
    assert_eq!(due_at_ns, LAUNCH_NS + 30 * SECOND_NS);
    assert!(
        supervisor
            .due_submitted_job_deadlines(due_at_ns - 1)
            .is_empty()
    );
    let due = supervisor.due_submitted_job_deadlines(due_at_ns);
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].job_id, job_id);

    let dispatch = supervisor
        .process_due_submitted_job_deadline(job_id, kind, &mut boundaries.controller, due_at_ns)
        .expect("timeout")
        .expect("acted");
    let SupervisorSubmittedDeadlineDispatch::Stop(stop) = dispatch else {
        panic!("expected a stop, got {dispatch:?}");
    };
    assert_eq!(stop.job_id, job_id);
    assert_eq!(stop.cause, SubmittedJobCause::Timeout);
    assert!(stop.signalled);
    assert_eq!(boundaries.controller.signals.len(), 1);
    assert_eq!(boundaries.controller.signals[0].target.pid, 9000);
    assert_eq!(
        boundaries.controller.signals[0].signal,
        ProcessSignal::Sigterm
    );
    assert_eq!(
        supervisor
            .submitted_job_view(job_id)
            .expect("view")
            .state
            .state,
        JobState::Running
    );

    // The termination grace: stop_timeout after the signal.
    let (kind, kill_at_ns) = deadline_kind(&supervisor);
    assert_eq!(kind, SubmittedJobDeadlineKind::StopKill);
    assert_eq!(kill_at_ns, due_at_ns + 5 * SECOND_NS);
    let dispatch = supervisor
        .process_due_submitted_job_deadline(job_id, kind, &mut boundaries.controller, kill_at_ns)
        .expect("kill")
        .expect("acted");
    assert_eq!(
        dispatch,
        SupervisorSubmittedDeadlineDispatch::Killed { job_id }
    );
    assert_eq!(boundaries.controller.cgroup_kills, vec![cgroup.clone()]);

    // The reap after the kill carries the cause the stop recorded.
    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Signaled {
            signal: libc::SIGKILL,
            core_dumped: false,
        },
        kill_at_ns + 1,
    );
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.state.state, JobState::Failed);
    assert_eq!(view.cause, Some(SubmittedJobCause::Timeout));
    assert!(supervisor.next_submitted_job_deadline().is_none());
}

#[test]
fn a_job_that_survives_the_kill_is_abandoned_after_the_post_kill_grace() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);
    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload(r#""timeout":1,"stop_timeout":1"#)),
    );
    launch(&mut supervisor, &mut boundaries.controller, 9000, 90);
    let cgroup = crate::job::submitted_job_cgroup_path(job_id);
    let timeout_at = LAUNCH_NS + SECOND_NS;
    supervisor
        .process_due_submitted_job_deadline(
            job_id,
            SubmittedJobDeadlineKind::Timeout,
            &mut boundaries.controller,
            timeout_at,
        )
        .expect("timeout");
    let kill_at = timeout_at + SECOND_NS;
    supervisor
        .process_due_submitted_job_deadline(
            job_id,
            SubmittedJobDeadlineKind::StopKill,
            &mut boundaries.controller,
            kill_at,
        )
        .expect("kill");

    let (kind, abandon_at) = deadline_kind(&supervisor);
    assert_eq!(kind, SubmittedJobDeadlineKind::PostKill);
    assert_eq!(
        abandon_at,
        kill_at + supervisor.settings().shutdown.post_kill_timeout_secs * SECOND_NS
    );
    boundaries
        .controller
        .set_cgroup_populated(cgroup.clone(), true);
    let dispatch = supervisor
        .process_due_submitted_job_deadline(job_id, kind, &mut boundaries.controller, abandon_at)
        .expect("post kill")
        .expect("acted");

    let SupervisorSubmittedDeadlineDispatch::Abandoned {
        job_event,
        cgroup_id,
    } = dispatch
    else {
        panic!("expected abandoned, got {dispatch:?}");
    };
    assert_eq!(job_event.job_id, job_id);
    assert_eq!(job_event.state, JobState::Abandoned);
    assert_eq!(cgroup_id, cgroup);
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.state.state, JobState::Abandoned);
    assert_eq!(view.cause, Some(SubmittedJobCause::ProcessUnkillable));
    assert!(supervisor.jobs().get(job_id).is_none());
}

#[test]
fn the_readiness_timeout_stops_a_job_that_never_said_ready() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);
    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload(
            r#""readiness":"notify","readiness_timeout":10"#,
        )),
    );
    launch(&mut supervisor, &mut boundaries.controller, 9000, 90);

    let (kind, due_at_ns) = deadline_kind(&supervisor);
    assert_eq!(kind, SubmittedJobDeadlineKind::ReadinessTimeout);
    assert_eq!(due_at_ns, LAUNCH_NS + 10 * SECOND_NS);
    let dispatch = supervisor
        .process_due_submitted_job_deadline(job_id, kind, &mut boundaries.controller, due_at_ns)
        .expect("readiness timeout")
        .expect("acted");

    let SupervisorSubmittedDeadlineDispatch::Stop(stop) = dispatch else {
        panic!("expected a stop, got {dispatch:?}");
    };
    assert_eq!(stop.cause, SubmittedJobCause::ReadinessTimeout);
    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Signaled {
            signal: libc::SIGTERM,
            core_dumped: false,
        },
        due_at_ns + 1,
    );
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.state.state, JobState::Failed);
    assert_eq!(view.cause, Some(SubmittedJobCause::ReadinessTimeout));
}

#[test]
fn a_deadline_for_a_job_that_already_ended_does_nothing() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);
    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload(r#""timeout":30"#)),
    );
    launch(&mut supervisor, &mut boundaries.controller, 9000, 90);
    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Exited { code: 0 },
        LAUNCH_NS + 1,
    );

    let dispatch = supervisor
        .process_due_submitted_job_deadline(
            job_id,
            SubmittedJobDeadlineKind::Timeout,
            &mut boundaries.controller,
            LAUNCH_NS + 30 * SECOND_NS,
        )
        .expect("stale deadline");

    assert_eq!(dispatch, None);
    assert!(boundaries.controller.signals.is_empty());
}
