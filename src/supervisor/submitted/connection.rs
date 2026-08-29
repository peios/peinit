//! One readiness turn on one jobs connection: read a record, run it, queue
//! the answer, flush, decide whether the connection stays.

use crate::boundary::{Clock, JobIdentityProvider, ProcessController, RealtimeClock};
use crate::jobs::connection::{JobsConnectionIo, JobsConnectionRecord, JobsPendingWait};
use crate::jobs::socket::{JobsSocketRead, JobsSocketReadError, JobsSocketWriteError};
use crate::submitted::{JobAccessChecker, JobAccessDenied, JobDescriptorFactory};

use super::commands::{SupervisorJobsMessageContext, SupervisorJobsMessageResponse};
use super::error::JobsCommandError;
use crate::supervisor::dispatch::SupervisorJobsCommandDispatch;
use crate::supervisor::state::Supervisor;

pub struct SupervisorJobsConnectionTurnContext<'a, I, D, P, C>
where
    I: JobIdentityProvider + ?Sized,
    D: JobDescriptorFactory + JobAccessChecker + ?Sized,
    P: ProcessController + ?Sized,
    C: Clock + RealtimeClock + ?Sized,
{
    pub identity_provider: &'a mut I,
    pub security: &'a mut D,
    pub controller: &'a mut P,
    pub clock: &'a mut C,
    pub max_message_bytes: usize,
    pub max_descriptors: usize,
    pub observed_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorJobsConnectionRead {
    Message {
        bytes: usize,
        attachments: usize,
    },
    Eof,
    WouldBlock,
    /// A wait is pending: nothing is read until it is answered.
    Waiting(JobsPendingWait),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorJobsConnectionTurn {
    pub read: SupervisorJobsConnectionRead,
    pub dispatch: Option<SupervisorJobsCommandDispatch>,
    pub error: Option<JobsCommandError>,
    pub access_denial: Option<JobAccessDenied>,
    pub wait: Option<JobsPendingWait>,
    pub sent: usize,
    pub pending: usize,
    pub close_connection: bool,
}

#[derive(Debug)]
pub enum SupervisorJobsConnectionTurnError {
    Read(JobsSocketReadError),
    Write(JobsSocketWriteError),
    Serialize(serde_json::Error),
}

impl Supervisor {
    pub fn process_jobs_connection_turn<K, I, D, P, C>(
        &mut self,
        connection: &mut JobsConnectionRecord<K>,
        context: SupervisorJobsConnectionTurnContext<'_, I, D, P, C>,
    ) -> Result<SupervisorJobsConnectionTurn, SupervisorJobsConnectionTurnError>
    where
        K: JobsConnectionIo,
        I: JobIdentityProvider + ?Sized,
        D: JobDescriptorFactory + JobAccessChecker + ?Sized,
        P: ProcessController + ?Sized,
        C: Clock + RealtimeClock + ?Sized,
    {
        let SupervisorJobsConnectionTurnContext {
            identity_provider,
            security,
            controller,
            clock,
            max_message_bytes,
            max_descriptors,
            observed_at_ns,
        } = context;

        let mut turn = SupervisorJobsConnectionTurn {
            read: SupervisorJobsConnectionRead::WouldBlock,
            dispatch: None,
            error: None,
            access_denial: None,
            wait: None,
            sent: 0,
            pending: 0,
            close_connection: false,
        };

        if let Some(wait) = connection.state().pending_wait() {
            turn.read = SupervisorJobsConnectionRead::Waiting(wait);
        } else {
            let read = connection
                .io_mut()
                .read_jobs(max_message_bytes, max_descriptors)
                .map_err(SupervisorJobsConnectionTurnError::Read)?;
            match read {
                JobsSocketRead::WouldBlock => {}
                JobsSocketRead::Eof => {
                    turn.read = SupervisorJobsConnectionRead::Eof;
                    turn.close_connection = true;
                }
                JobsSocketRead::Message(message) => {
                    turn.read = SupervisorJobsConnectionRead::Message {
                        bytes: message.payload.len(),
                        attachments: message.descriptors.len()
                            + usize::from(message.token.is_some()),
                    };
                    let (peer, state) = connection.peer_and_state_mut();
                    let SupervisorJobsMessageResponse {
                        response,
                        wait,
                        close_after,
                        dispatch,
                        access_denial,
                        error,
                    } = self
                        .run_jobs_message(
                            peer,
                            message,
                            SupervisorJobsMessageContext {
                                identity_provider,
                                security,
                                controller,
                                clock,
                            },
                        )
                        .map_err(SupervisorJobsConnectionTurnError::Serialize)?;
                    if let Some(frame) = response {
                        state.enqueue(frame.bytes, frame.fd, close_after);
                    }
                    if let Some(wait) = wait {
                        state.set_pending_wait(wait);
                    }
                    turn.wait = wait;
                    turn.dispatch = dispatch;
                    turn.access_denial = access_denial;
                    turn.error = error;
                    state.mark_activity(observed_at_ns);
                }
            }
        }

        let flush = connection
            .flush()
            .map_err(SupervisorJobsConnectionTurnError::Write)?;
        turn.sent = flush.sent;
        turn.pending = flush.pending;
        if flush.sent > 0 {
            connection.state_mut().mark_activity(observed_at_ns);
        }
        if flush.close_after_write && flush.pending == 0 {
            turn.close_connection = true;
        }
        Ok(turn)
    }
}
