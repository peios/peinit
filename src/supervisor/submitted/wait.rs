//! Answering the messages held on jobs connections once what they wait for
//! has happened — run once per turn, like the control channel's waits.

use crate::control::wire::ControlResponseTimeProjection;
use crate::jobs::connection::{
    JobsConnectionIo, JobsConnectionRecord, JobsConnectionTable, JobsPendingWait,
};
use crate::jobs::socket::JobsSocketWriteError;
use crate::jobs::wire::jobs_error_response;

use crate::supervisor::state::Supervisor;

impl Supervisor {
    pub fn flush_jobs_waits<I>(
        &self,
        connections: &mut JobsConnectionTable<I>,
        time: ControlResponseTimeProjection,
        observed_at_ns: u64,
    ) -> Result<SupervisorJobsWaitFlushTurn, SupervisorJobsWaitFlushError>
    where
        I: JobsConnectionIo,
    {
        let mut completed = Vec::new();
        for fd in connections.fds() {
            let Some(connection) = connections.get_mut(fd) else {
                continue;
            };
            let Some(wait) = connection.state().pending_wait() else {
                continue;
            };
            if !self.jobs_pending_wait_ready(wait) {
                continue;
            }
            self.answer_jobs_wait(connection, wait, time, observed_at_ns)?;
            completed.push(SupervisorJobsWaitFlush { fd, wait });
        }
        Ok(SupervisorJobsWaitFlushTurn { completed })
    }

    fn jobs_pending_wait_ready(&self, wait: JobsPendingWait) -> bool {
        match wait {
            JobsPendingWait::Submit { job_id } => {
                self.submitted_job_left_created(job_id).unwrap_or(true)
            }
            JobsPendingWait::Wait { job_id, condition } => {
                self.jobs_wait_satisfied(job_id, condition)
            }
            JobsPendingWait::Stop { job_id } => self.submitted_job_terminal(job_id),
        }
    }

    fn answer_jobs_wait<I>(
        &self,
        connection: &mut JobsConnectionRecord<I>,
        wait: JobsPendingWait,
        time: ControlResponseTimeProjection,
        observed_at_ns: u64,
    ) -> Result<(), SupervisorJobsWaitFlushError>
    where
        I: JobsConnectionIo,
    {
        let job_id = wait.job_id();
        // Only the submit answer carries the process handle (TRM §10.7); a
        // wait or stop that resolves later is answered with the bare view.
        let frame = match wait {
            JobsPendingWait::Submit { .. } => self.jobs_view_frame_with_handle(job_id, time),
            JobsPendingWait::Wait { .. } | JobsPendingWait::Stop { .. } => {
                self.jobs_view_frame(job_id, time)
            }
        };
        // Whatever went wrong answering this wait is this connection's
        // answer — an error record — never a reason to leave every other
        // wait unanswered.
        let (bytes, fd) = match frame {
            Ok(frame) => (frame.bytes, frame.fd),
            Err(error) => (
                jobs_error_response(error.code(), &error.message())
                    .map_err(|error| SupervisorJobsWaitFlushError::Response(error.to_string()))?,
                None,
            ),
        };
        connection.state_mut().clear_pending_wait();
        connection.state_mut().enqueue(bytes, fd, false);
        let flush = connection
            .flush()
            .map_err(SupervisorJobsWaitFlushError::Write)?;
        if flush.sent > 0 {
            connection.state_mut().mark_activity(observed_at_ns);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorJobsWaitFlushTurn {
    pub completed: Vec<SupervisorJobsWaitFlush>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorJobsWaitFlush {
    pub fd: i32,
    pub wait: JobsPendingWait,
}

#[derive(Debug)]
pub enum SupervisorJobsWaitFlushError {
    Response(String),
    Write(JobsSocketWriteError),
}
