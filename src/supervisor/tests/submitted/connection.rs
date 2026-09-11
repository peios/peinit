//! The jobs connection turn: a record with scripted reads, the message
//! run through the supervisor, and the wait answered by the flush.

use std::collections::VecDeque;
use std::os::fd::{AsRawFd, IntoRawFd};

use crate::boundary::ChildExitStatus;
use crate::control::connection::ControlConnectionTable;
use crate::control::wire::ControlResponseTimeProjection;
use crate::jobs::connection::{JobsConnectionIo, JobsConnectionRecord, JobsPendingWait};
use crate::jobs::socket::{
    JobsMessage, JobsSocketRead, JobsSocketReadError, JobsSocketWrite, JobsSocketWriteError,
};
use crate::jobs::wire::JobsWaitCondition;
use crate::supervisor::{
    SupervisorJobsConnectionRead, SupervisorJobsConnectionTurnContext, SupervisorJobsWaitFlush,
};

use super::support::{
    Boundaries, LAUNCH_NS, SUBMIT_NS, SUBMITTER, jobs_peer, launch, message, reap, response_json,
    submit_payload, submitted_supervisor,
};

#[derive(Debug, Default)]
struct FakeJobsIo {
    reads: VecDeque<JobsSocketRead>,
    writes: Vec<(Vec<u8>, Option<i32>)>,
}

impl FakeJobsIo {
    fn reads(reads: impl IntoIterator<Item = JobsMessage>) -> Self {
        Self {
            reads: reads.into_iter().map(JobsSocketRead::Message).collect(),
            writes: Vec::new(),
        }
    }
}

impl JobsConnectionIo for FakeJobsIo {
    fn read_jobs(
        &mut self,
        _max_bytes: usize,
        _max_descriptors: usize,
    ) -> Result<JobsSocketRead, JobsSocketReadError> {
        Ok(self.reads.pop_front().unwrap_or(JobsSocketRead::WouldBlock))
    }

    fn write_jobs(
        &mut self,
        bytes: &[u8],
        fd: Option<i32>,
    ) -> Result<JobsSocketWrite, JobsSocketWriteError> {
        self.writes.push((bytes.to_vec(), fd));
        Ok(JobsSocketWrite::Complete)
    }
}

fn turn(
    supervisor: &mut crate::supervisor::Supervisor,
    boundaries: &mut Boundaries,
    connection: &mut JobsConnectionRecord<FakeJobsIo>,
    observed_at_ns: u64,
) -> crate::supervisor::SupervisorJobsConnectionTurn {
    supervisor
        .process_jobs_connection_turn(
            connection,
            SupervisorJobsConnectionTurnContext {
                identity_provider: &mut boundaries.identity,
                security: &mut boundaries.security,
                controller: &mut boundaries.controller,
                clock: &mut boundaries.clock,
                max_message_bytes: 65536,
                max_descriptors: 8,
                observed_at_ns,
            },
        )
        .expect("connection turn")
}

fn time(now_ns: u64) -> ControlResponseTimeProjection {
    ControlResponseTimeProjection::new(now_ns, super::super::TEST_REALTIME_NS)
}

#[test]
fn submit_is_answered_with_the_pidfd_once_the_job_is_running() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, SUBMIT_NS + 1]);
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            70,
            JobsConnectionRecord::new_with_activity(
                FakeJobsIo::reads([message(&submit_payload(""))]),
                jobs_peer(SUBMITTER),
                Some(SUBMIT_NS),
            ),
        )
        .expect("admit");

    let read = turn(
        &mut supervisor,
        &mut boundaries,
        connections.get_mut(70).expect("record"),
        SUBMIT_NS,
    );
    assert!(matches!(
        read.read,
        SupervisorJobsConnectionRead::Message { .. }
    ));
    let Some(JobsPendingWait::Submit { job_id }) = read.wait else {
        panic!("expected a submit wait, got {:?}", read.wait);
    };
    assert_eq!(read.sent, 0);
    assert!(!read.close_connection);
    assert!(connections.has_pending_jobs_waits());

    // Nothing is read while the wait is pending, and the flush has nothing
    // to say until the job leaves created.
    let waiting = turn(
        &mut supervisor,
        &mut boundaries,
        connections.get_mut(70).expect("record"),
        SUBMIT_NS + 1,
    );
    assert_eq!(
        waiting.read,
        SupervisorJobsConnectionRead::Waiting(JobsPendingWait::Submit { job_id })
    );
    let flush = supervisor
        .flush_jobs_waits(&mut connections, time(SUBMIT_NS + 1), SUBMIT_NS + 1)
        .expect("flush");
    assert!(flush.completed.is_empty());

    // A real descriptor stands in for the pidfd: the answer duplicates it.
    let pidfd = super::support::dev_null_fd().into_raw_fd();
    launch(&mut supervisor, &mut boundaries.controller, 9000, pidfd);
    let flush = supervisor
        .flush_jobs_waits(&mut connections, time(LAUNCH_NS), LAUNCH_NS)
        .expect("flush");
    assert_eq!(
        flush.completed,
        vec![SupervisorJobsWaitFlush {
            fd: 70,
            wait: JobsPendingWait::Submit { job_id },
        }]
    );
    let record = connections.get_mut(70).expect("record");
    assert!(record.state().pending_wait().is_none());
    // The flush answers on the spot: the response, with the pidfd, is
    // already on the wire and the next turn has nothing left to send.
    let (bytes, fd) = record.io_mut().writes[0].clone();
    let bytes = &bytes;
    let fd = &fd;
    let json = response_json(bytes);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["job"]["id"], job_id.to_canonical_string());
    assert_eq!(json["job"]["state"], "running");
    assert_eq!(json["job"]["pid"], 9000);
    assert!(fd.is_some(), "the pidfd rides with the submit response");
    assert_ne!(fd.unwrap(), pidfd, "a duplicate, not peinit's own handle");
}

#[test]
fn a_failed_launch_answers_the_submit_with_the_terminal_view_and_no_fd() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS]);
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            70,
            JobsConnectionRecord::new_with_activity(
                FakeJobsIo::reads([message(&submit_payload(""))]),
                jobs_peer(SUBMITTER),
                Some(SUBMIT_NS),
            ),
        )
        .expect("admit");
    turn(
        &mut supervisor,
        &mut boundaries,
        connections.get_mut(70).expect("record"),
        SUBMIT_NS,
    );
    let mut tokens = super::support::SubmittedTokenProvider {
        fail_materialisation: true,
        ..Default::default()
    };
    let mut launcher = super::super::TestProcessLauncher::new(Vec::new());
    let mut clock = super::super::ScriptedClock::new([LAUNCH_NS]);
    supervisor
        .launch_next_pending_submitted_job(
            &mut tokens,
            &mut launcher,
            &mut clock,
            &mut boundaries.controller,
        )
        .expect("launch");

    let flush = supervisor
        .flush_jobs_waits(&mut connections, time(LAUNCH_NS), LAUNCH_NS)
        .expect("flush");
    assert_eq!(flush.completed.len(), 1);
    let record = connections.get_mut(70).expect("record");
    let (bytes, fd) = &record.io_mut().writes[0];
    let json = response_json(bytes);
    assert_eq!(
        json["status"], "ok",
        "a refused launch is a terminal job, not a protocol error"
    );
    assert_eq!(json["job"]["state"], "failed");
    assert_eq!(json["job"]["cause"], "parent_setup_failure");
    assert_eq!(*fd, None);
}

#[test]
fn wait_for_terminal_is_answered_by_the_reap() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, LAUNCH_NS + 1]);
    let job_id = super::support::running_job(&mut supervisor, &mut boundaries, 9000, 90);
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            71,
            JobsConnectionRecord::new_with_activity(
                FakeJobsIo::reads([message(&format!(
                    r#"{{"command":"wait","job_id":"{job_id}","for":"terminal"}}"#
                ))]),
                jobs_peer(SUBMITTER),
                Some(LAUNCH_NS),
            ),
        )
        .expect("admit");

    let read = turn(
        &mut supervisor,
        &mut boundaries,
        connections.get_mut(71).expect("record"),
        LAUNCH_NS + 1,
    );
    assert_eq!(
        read.wait,
        Some(JobsPendingWait::Wait {
            job_id,
            condition: JobsWaitCondition::Terminal,
        })
    );
    reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Exited { code: 0 },
        LAUNCH_NS + 2,
    );
    let flush = supervisor
        .flush_jobs_waits(&mut connections, time(LAUNCH_NS + 2), LAUNCH_NS + 2)
        .expect("flush");
    assert_eq!(flush.completed.len(), 1);
    let record = connections.get_mut(71).expect("record");
    let json = response_json(&record.io_mut().writes[0].0);
    assert_eq!(json["job"]["state"], "completed");
    assert_eq!(json["job"]["exit_code"], 0);
}

#[test]
fn wait_for_ready_on_a_job_without_a_readiness_protocol_is_invalid_state() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, LAUNCH_NS + 1]);
    let job_id = super::support::running_job(&mut supervisor, &mut boundaries, 9000, 90);
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            72,
            JobsConnectionRecord::new_with_activity(
                FakeJobsIo::reads([message(&format!(
                    r#"{{"command":"wait","job_id":"{job_id}","for":"ready"}}"#
                ))]),
                jobs_peer(SUBMITTER),
                Some(LAUNCH_NS),
            ),
        )
        .expect("admit");

    let read = turn(
        &mut supervisor,
        &mut boundaries,
        connections.get_mut(72).expect("record"),
        LAUNCH_NS + 1,
    );

    assert_eq!(read.wait, None);
    assert!(matches!(
        read.error,
        Some(crate::supervisor::JobsCommandError::InvalidState { .. })
    ));
    assert_eq!(read.sent, 1);
    let json = response_json(&connections.get_mut(72).expect("record").io_mut().writes[0].0);
    assert_eq!(json["status"], "error");
    assert_eq!(
        json["code"],
        crate::jobs::wire::JobsErrorCode::InvalidState.as_str()
    );
}

#[test]
fn eof_closes_the_connection() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([]);
    let mut record = JobsConnectionRecord::new_with_activity(
        FakeJobsIo {
            reads: VecDeque::from([JobsSocketRead::Eof]),
            writes: Vec::new(),
        },
        jobs_peer(SUBMITTER),
        Some(SUBMIT_NS),
    );

    let read = turn(&mut supervisor, &mut boundaries, &mut record, SUBMIT_NS);

    assert_eq!(read.read, SupervisorJobsConnectionRead::Eof);
    assert!(read.close_connection);
    let _ = record.peer().pidfd.as_raw_fd();
}

/// §10.7: an answer that cannot be built — a job whose record was purged
/// before the flush, or a pidfd that cannot be duplicated — becomes that one
/// connection's error record and never aborts the flush of every other wait.
///
/// No guest can time two waits so that one's record is purged at the instant
/// of the flush while another's is answerable; here two pending waits are set
/// directly, the first on a job id with no entry (the purged case: its view
/// cannot be built, so it is `UNKNOWN_JOB`) and the second on a job that has
/// gone terminal and is answerable. The purged one is ordered first, so a
/// flush that gave up on it would never reach the second.
#[test]
fn an_unbuildable_answer_does_not_abort_the_flush_of_the_others() {
    let mut supervisor = submitted_supervisor();
    let mut boundaries = Boundaries::at([SUBMIT_NS, LAUNCH_NS + 1]);
    let live = super::support::running_job(&mut supervisor, &mut boundaries, 9000, 90);
    super::support::reap(
        &mut supervisor,
        &mut boundaries.controller,
        9000,
        ChildExitStatus::Exited { code: 0 },
        LAUNCH_NS + 2,
    );

    let purged = crate::ids::JobId::parse_canonical_str("00000000-0000-7000-8000-000000000000")
        .expect("a well-formed but unknown job id");

    let mut connections = ControlConnectionTable::new(4);
    for (fd, job_id) in [(80, purged), (81, live)] {
        let mut record = JobsConnectionRecord::new_with_activity(
            FakeJobsIo::default(),
            jobs_peer(SUBMITTER),
            Some(LAUNCH_NS + 2),
        );
        record.state_mut().set_pending_wait(JobsPendingWait::Wait {
            job_id,
            condition: JobsWaitCondition::Terminal,
        });
        connections.admit(fd, record).expect("admit");
    }

    let flush = supervisor
        .flush_jobs_waits(&mut connections, time(LAUNCH_NS + 3), LAUNCH_NS + 3)
        .expect("flush");
    assert_eq!(
        flush.completed.len(),
        2,
        "both waits were answered; the unbuildable one did not abort the flush",
    );

    // The purged wait got this connection's error record...
    let purged_json = response_json(&connections.get_mut(80).expect("record").io_mut().writes[0].0);
    assert_eq!(purged_json["status"], "error");
    assert_eq!(
        purged_json["code"],
        crate::jobs::wire::JobsErrorCode::UnknownJob.as_str(),
    );
    // ...and the answerable one got its terminal view all the same.
    let live_json = response_json(&connections.get_mut(81).expect("record").io_mut().writes[0].0);
    assert_eq!(live_json["job"]["state"], "completed");
}
