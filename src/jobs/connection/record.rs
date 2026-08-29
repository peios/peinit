use std::os::fd::AsRawFd;

use crate::jobs::socket::{
    JobsSocketRead, JobsSocketReadError, JobsSocketWrite, JobsSocketWriteError,
    LinuxJobsConnection,
};

use super::accept::JobsPeer;
use super::state::JobsConnectionState;

pub trait JobsConnectionIo {
    fn read_jobs(
        &mut self,
        max_bytes: usize,
        max_descriptors: usize,
    ) -> Result<JobsSocketRead, JobsSocketReadError>;

    fn write_jobs(
        &mut self,
        bytes: &[u8],
        fd: Option<i32>,
    ) -> Result<JobsSocketWrite, JobsSocketWriteError>;
}

impl JobsConnectionIo for LinuxJobsConnection {
    fn read_jobs(
        &mut self,
        max_bytes: usize,
        max_descriptors: usize,
    ) -> Result<JobsSocketRead, JobsSocketReadError> {
        self.receive(max_bytes, max_descriptors)
    }

    fn write_jobs(
        &mut self,
        bytes: &[u8],
        fd: Option<i32>,
    ) -> Result<JobsSocketWrite, JobsSocketWriteError> {
        self.send(bytes, fd)
    }
}

#[derive(Debug)]
pub struct JobsConnectionRecord<I> {
    io: I,
    peer: JobsPeer,
    state: JobsConnectionState,
}

impl<I> JobsConnectionRecord<I> {
    pub fn new_with_activity(io: I, peer: JobsPeer, observed_at_ns: Option<u64>) -> Self {
        let mut state = JobsConnectionState::new();
        if let Some(observed_at_ns) = observed_at_ns {
            state.mark_activity(observed_at_ns);
        }
        Self { io, peer, state }
    }

    pub fn peer(&self) -> &JobsPeer {
        &self.peer
    }

    pub fn state(&self) -> &JobsConnectionState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut JobsConnectionState {
        &mut self.state
    }

    pub fn peer_and_state_mut(&mut self) -> (&JobsPeer, &mut JobsConnectionState) {
        let Self { peer, state, .. } = self;
        (peer, state)
    }

    pub fn io_mut(&mut self) -> &mut I {
        &mut self.io
    }
}

impl<I> JobsConnectionRecord<I>
where
    I: JobsConnectionIo,
{
    /// Send queued records until one would block. Each record goes whole.
    pub fn flush(&mut self) -> Result<JobsConnectionFlush, JobsSocketWriteError> {
        let mut sent = 0;
        while let Some(front) = self.state.front() {
            let fd = front.fd.as_ref().map(AsRawFd::as_raw_fd);
            match self.io.write_jobs(&front.bytes, fd)? {
                JobsSocketWrite::Complete => {
                    self.state.pop_front();
                    sent += 1;
                }
                JobsSocketWrite::WouldBlock => {
                    return Ok(JobsConnectionFlush {
                        sent,
                        pending: self.state.pending_messages(),
                        close_after_write: self.state.close_after_write(),
                    });
                }
            }
        }
        Ok(JobsConnectionFlush {
            sent,
            pending: 0,
            close_after_write: self.state.close_after_write(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobsConnectionFlush {
    pub sent: usize,
    pub pending: usize,
    pub close_after_write: bool,
}
