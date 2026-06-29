use crate::job::{JobExit, JobState, JobTransitionAction, JobTransitionError, ProcessHandle};

use super::super::service_main_job;

#[test]
fn running_job_can_complete_with_exit_code() {
    let mut job = service_main_job();

    job.start(
        ProcessHandle {
            pid: 1234,
            pidfd: 9,
        },
        1_100,
    )
    .expect("start");
    job.complete(1_500, 0).expect("complete");

    assert_eq!(job.state, JobState::Completed);
    assert_eq!(job.pid, Some(1234));
    assert_eq!(job.pidfd, Some(9));
    assert_eq!(job.started_at_ns, Some(1_100));
    assert_eq!(job.ended_at_ns, Some(1_500));
    assert_eq!(job.exit_code, Some(0));
    assert_eq!(job.exit_signal, None);
    assert_eq!(job.duration_ns(), Some(500));
    assert!(job.state.is_terminal());
}

#[test]
fn created_job_can_fail_before_fork_without_process_fields() {
    let mut job = service_main_job();

    job.fail_before_start(1_050, "ParentSetupFailure: cgroup")
        .expect("fail before start");

    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.started_at_ns, None);
    assert_eq!(job.pid, None);
    assert_eq!(job.pidfd, None);
    assert_eq!(job.exit_code, None);
    assert_eq!(job.exit_signal, None);
    assert_eq!(
        job.failure_cause.as_deref(),
        Some("ParentSetupFailure: cgroup"),
    );
}

#[test]
fn running_job_can_fail_or_be_abandoned_with_terminal_evidence() {
    let mut failed = service_main_job();
    failed
        .start(
            ProcessHandle {
                pid: 1234,
                pidfd: 9,
            },
            1_100,
        )
        .expect("start");
    failed
        .fail_running(1_300, Some(JobExit::Signal(9)), "signal")
        .expect("fail");
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.exit_signal, Some(9));
    assert_eq!(failed.failure_cause.as_deref(), Some("signal"));

    let mut abandoned = service_main_job();
    abandoned
        .start(
            ProcessHandle {
                pid: 5678,
                pidfd: 10,
            },
            1_100,
        )
        .expect("start");
    abandoned
        .abandon(1_400, "ProcessUnkillable")
        .expect("abandon");
    assert_eq!(abandoned.state, JobState::Abandoned);
    assert_eq!(abandoned.exit_code, None);
    assert_eq!(abandoned.exit_signal, None);
    assert_eq!(
        abandoned.failure_cause.as_deref(),
        Some("ProcessUnkillable"),
    );
}

#[test]
fn invalid_job_transition_does_not_mutate_record() {
    let mut job = service_main_job();
    let before = job.clone();

    let err = job.complete(1_200, 0).expect_err("invalid");

    assert_eq!(job, before);
    assert_eq!(
        err,
        JobTransitionError::InvalidTransition {
            id: before.id,
            from: JobState::Created,
            action: JobTransitionAction::Complete,
        }
    );
}

#[test]
fn job_timestamps_cannot_move_backwards() {
    let mut job = service_main_job();
    let err = job
        .start(ProcessHandle { pid: 1, pidfd: 2 }, 999)
        .expect_err("start before creation");
    assert_eq!(
        err,
        JobTransitionError::StartBeforeCreation {
            id: job.id,
            created_at_ns: 1_000,
            started_at_ns: 999,
        }
    );
    assert_eq!(job.state, JobState::Created);

    job.start(ProcessHandle { pid: 1, pidfd: 2 }, 1_100)
        .expect("start");
    let err = job.complete(1_050, 0).expect_err("end before start");
    assert_eq!(
        err,
        JobTransitionError::EndBeforeStart {
            id: job.id,
            started_at_ns: 1_100,
            ended_at_ns: 1_050,
        }
    );
    assert_eq!(job.state, JobState::Running);
}
