use crate::boundary::ChildExitStatus;
use crate::job::JobState;
use crate::submitted::SubmittedJobCause;
use crate::supervisor::{
    SupervisorChildReapDispatch, SupervisorChildReapTurn, SupervisorSubmittedLaunchResult,
};

use super::support::{
    Boundaries, IDENTITY_SID, LAUNCH_NS, SUBMIT_NS, SUBMITTER, jobs_peer, launch, message,
    message_with, reap, running_job, submit, submit_payload, submitted_supervisor,
};

const END_NS: u64 = LAUNCH_NS + 5_000_000_000;

#[test]
fn launch_runs_the_job_as_its_prepared_token_with_its_descriptors() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);
    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message_with(
            &submit_payload(
                r#""descriptors":["control"],"output":true,"environment":{"MODE":"full"}"#,
            ),
            false,
            2,
        ),
    );
    let prepared_fd = supervisor
        .submitted_jobs()
        .get(job_id)
        .expect("entry")
        .prepared_token_fd
        .expect("prepared token");

    let (result, tokens, launcher) = launch(&mut supervisor, &mut boundaries.controller, 9000, 90);

    let SupervisorSubmittedLaunchResult::Launched(dispatch) = result else {
        panic!("expected launched, got {result:?}");
    };
    assert_eq!(dispatch.launch.job_event.job_id, job_id);
    assert_eq!(dispatch.launch.job_event.state, JobState::Running);
    assert_eq!(dispatch.launch.job_event.pid, Some(9000));
    assert!(
        dispatch.output_sink_fd.is_some(),
        "the sink goes to the runtime"
    );
    assert_eq!(tokens.prepared_token_fds, vec![prepared_fd]);
    assert_eq!(
        launcher.observed_inherited_fd_names,
        vec![vec!["control".to_string()]]
    );
    assert_eq!(launcher.observed_listen_fds, vec![Some("1".to_string())]);
    assert_eq!(
        launcher.observed_listen_fdnames,
        vec![Some("control".to_string())]
    );

    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.state.state, JobState::Running);
    assert_eq!(view.state.pid, Some(9000));
    assert_eq!(view.state.started_at_ns, Some(LAUNCH_NS));
    assert_eq!(view.identity_sid, IDENTITY_SID);
    assert!(supervisor.pending_submitted_launch_jobs().is_empty());
    let entry = supervisor.submitted_jobs().get(job_id).expect("entry");
    assert_eq!(
        entry.prepared_token_fd, None,
        "the launch consumed the token"
    );
    assert!(
        entry.attached_descriptors.is_empty(),
        "the launch consumed the descriptors"
    );
    assert_eq!(entry.output_sink_fd, None, "the runtime owns the sink");
}

#[test]
fn a_launch_failure_is_terminal_with_parent_setup_failure() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let peer = jobs_peer(SUBMITTER);
    let job_id = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload("")),
    );
    let mut tokens = super::support::SubmittedTokenProvider {
        fail_materialisation: true,
        ..Default::default()
    };
    let mut launcher = super::super::TestProcessLauncher::new(Vec::new());
    let mut clock = super::super::ScriptedClock::new([LAUNCH_NS]);

    let result = supervisor
        .launch_next_pending_submitted_job(
            &mut tokens,
            &mut launcher,
            &mut clock,
            &mut boundaries.controller,
        )
        .expect("launch")
        .expect("queued launch");

    let SupervisorSubmittedLaunchResult::Failed(failure) = result else {
        panic!("expected failure, got {result:?}");
    };
    assert_eq!(failure.job_event.job_id, job_id);
    assert_eq!(failure.job_event.state, JobState::Failed);
    assert_eq!(failure.cause, SubmittedJobCause::ParentSetupFailure);
    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.state.state, JobState::Failed);
    assert_eq!(view.cause, Some(SubmittedJobCause::ParentSetupFailure));
    assert!(launcher.observed_jobs.is_empty(), "no process was launched");
}

#[test]
fn a_zero_exit_completes_the_job_and_retains_the_view() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);

    let turn = reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Exited { code: 0 },
        END_NS,
    );

    let SupervisorChildReapTurn::Tracked {
        dispatch: SupervisorChildReapDispatch::Submitted(terminal),
        ..
    } = turn
    else {
        panic!("expected a submitted reap, got {turn:?}");
    };
    assert_eq!(terminal.job_event.state, JobState::Completed);
    assert_eq!(terminal.cause, None);
    assert!(!terminal.cgroup_busy);
    let view = supervisor
        .submitted_job_view(job_id)
        .expect("retained view");
    assert_eq!(view.state.state, JobState::Completed);
    assert_eq!(view.state.exit_code, Some(0));
    assert_eq!(view.state.ended_at_ns, Some(END_NS));
    assert_eq!(view.state.pid, None, "no process once terminal (PSPU §7.7)");
    assert!(
        supervisor.jobs().get(job_id).is_none(),
        "the record is dropped once terminal"
    );
    assert_eq!(
        boundaries.controller.cgroup_removes,
        vec![crate::job::submitted_job_cgroup_path(job_id)]
    );
}

#[test]
fn a_non_success_exit_fails_the_job_unless_listed() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, SUBMIT_NS + 1]);
    let peer = jobs_peer(SUBMITTER);
    let plain = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload("")),
    );
    launch(&mut supervisor, &mut boundaries.controller, 9000, 90);
    let lenient = submit(
        &mut supervisor,
        &mut boundaries,
        &peer,
        message(&submit_payload(r#""success_exit_codes":[0,3]"#)),
    );
    launch(&mut supervisor, &mut boundaries.controller, 9001, 91);

    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Exited { code: 3 },
        END_NS,
    );
    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9001,
        ChildExitStatus::Exited { code: 3 },
        END_NS,
    );

    let plain_view = supervisor.submitted_job_view(plain).expect("view");
    assert_eq!(plain_view.state.state, JobState::Failed);
    assert_eq!(plain_view.state.exit_code, Some(3));
    assert_eq!(
        plain_view.cause, None,
        "an ordinary failure has no peinit cause"
    );
    let lenient_view = supervisor.submitted_job_view(lenient).expect("view");
    assert_eq!(lenient_view.state.state, JobState::Completed);
}

#[test]
fn a_signalled_exit_records_the_signal() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);

    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Signaled {
            signal: libc::SIGKILL,
            core_dumped: false,
        },
        END_NS,
    );

    let view = supervisor.submitted_job_view(job_id).expect("view");
    assert_eq!(view.state.state, JobState::Failed);
    assert_eq!(view.state.exit_signal, Some(libc::SIGKILL));
    assert_eq!(view.state.exit_code, None);
}

#[test]
fn a_terminal_job_is_forgotten_after_the_retention_window() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);
    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Exited { code: 0 },
        END_NS,
    );
    let retention = crate::submitted::DEFAULT_SUBMITTED_JOB_RETENTION_NS;

    let early = supervisor
        .process_due_operation_maintenance(END_NS + retention - 1)
        .expect("maintenance");
    assert!(early.purged_jobs.is_empty());
    assert!(supervisor.submitted_job_view(job_id).is_some());

    let due = supervisor
        .process_due_operation_maintenance(END_NS + retention)
        .expect("maintenance");
    assert_eq!(due.purged_jobs, vec![job_id]);
    assert!(supervisor.submitted_job_view(job_id).is_none());
}

#[test]
fn a_busy_cgroup_at_exit_schedules_a_cleanup_retry() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let job_id = running_job(&mut supervisor, &mut boundaries, 9000, 90);
    let cgroup = crate::job::submitted_job_cgroup_path(job_id);
    boundaries
        .controller
        .set_cgroup_remove_result(cgroup.clone(), crate::boundary::CgroupRemoveOutcome::Busy);

    let turn = reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Exited { code: 0 },
        END_NS,
    );

    let SupervisorChildReapTurn::Tracked {
        dispatch: SupervisorChildReapDispatch::Submitted(terminal),
        ..
    } = turn
    else {
        panic!("expected a submitted reap");
    };
    assert!(terminal.cgroup_busy);
    let deadline = supervisor
        .next_submitted_job_deadline()
        .expect("cleanup retry");
    assert!(matches!(
        deadline.kind,
        crate::supervisor::SupervisorLifecycleDeadlineKind::SubmittedJob {
            kind: crate::submitted::SubmittedJobDeadlineKind::CgroupCleanup,
            ..
        }
    ));
}
