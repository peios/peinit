//! The jobs-channel commands (PSPU §7.4, §7.8), run against one received
//! message on one connection.

use std::os::fd::{BorrowedFd, OwnedFd};

use crate::boundary::{
    Clock, JobIdentityProvider, ProcessController, ProcessSignal, RealtimeClock,
};
use crate::control::wire::ControlResponseTimeProjection;
use crate::ids::JobId;
use crate::job::JobState;
use crate::jobs::connection::{JobsPeer, JobsPendingWait};
use crate::jobs::socket::JobsMessage;
use crate::jobs::wire::{
    JobsCommand, JobsWaitCondition, ParsedJobsRequest, jobs_error_response, jobs_job_response,
    parse_jobs_request,
};
use crate::submitted::{
    JobAccess, JobAccessCheckRequest, JobAccessChecker, JobAccessDenied, JobDescriptorFactory,
    JobReadiness, SubmittedJobCause, job_view_json,
};

use super::error::JobsCommandError;
use super::stop::{SubmittedStopOutcome, begin_submitted_stop, submitted_process_target};
use crate::supervisor::dispatch::SupervisorJobsCommandDispatch;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

pub struct SupervisorJobsMessageContext<'a, I, D, P, C>
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
}

/// One record to send back, with the descriptor it carries.
#[derive(Debug)]
pub struct JobsResponseFrame {
    pub bytes: Vec<u8>,
    pub fd: Option<OwnedFd>,
}

#[derive(Debug)]
pub struct SupervisorJobsMessageResponse {
    /// The immediate response, if the command has one now.
    pub response: Option<JobsResponseFrame>,
    /// The connection is to wait on this before answering.
    pub wait: Option<JobsPendingWait>,
    pub close_after: bool,
    pub dispatch: Option<SupervisorJobsCommandDispatch>,
    pub access_denial: Option<JobAccessDenied>,
    pub error: Option<JobsCommandError>,
}

impl Supervisor {
    /// Run the message a jobs connection received. Every path either
    /// answers now, sets a wait, or both refuses and answers; and every
    /// attachment the message carried is consumed by the time it returns.
    pub fn run_jobs_message<I, D, P, C>(
        &mut self,
        peer: &JobsPeer,
        message: JobsMessage,
        context: SupervisorJobsMessageContext<'_, I, D, P, C>,
    ) -> Result<SupervisorJobsMessageResponse, serde_json::Error>
    where
        I: JobIdentityProvider + ?Sized,
        D: JobDescriptorFactory + JobAccessChecker + ?Sized,
        P: ProcessController + ?Sized,
        C: Clock + RealtimeClock + ?Sized,
    {
        let mut denial = None;
        let outcome = self.run_jobs_message_inner(peer, message, context, &mut denial);
        Ok(match outcome {
            Ok(mut response) => {
                response.access_denial = denial;
                response
            }
            Err(error) => SupervisorJobsMessageResponse {
                response: Some(JobsResponseFrame {
                    bytes: jobs_error_response(error.code(), &error.message())?,
                    fd: None,
                }),
                wait: None,
                close_after: error.closes_connection(),
                dispatch: None,
                access_denial: denial,
                error: Some(error),
            },
        })
    }

    fn run_jobs_message_inner<I, D, P, C>(
        &mut self,
        peer: &JobsPeer,
        message: JobsMessage,
        context: SupervisorJobsMessageContext<'_, I, D, P, C>,
        denial: &mut Option<JobAccessDenied>,
    ) -> Result<SupervisorJobsMessageResponse, JobsCommandError>
    where
        I: JobIdentityProvider + ?Sized,
        D: JobDescriptorFactory + JobAccessChecker + ?Sized,
        P: ProcessController + ?Sized,
        C: Clock + RealtimeClock + ?Sized,
    {
        if message.truncated {
            return Err(JobsCommandError::Truncated);
        }
        if message.control_truncated {
            return Err(JobsCommandError::ControlTruncated);
        }
        let parsed = parse_jobs_request(&message.payload).map_err(JobsCommandError::Parse)?;
        let now_ns = context
            .clock
            .monotonic_ns()
            .map_err(|error| JobsCommandError::Internal(format!("{error:?}")))?;
        let time = time_projection(context.clock, now_ns)?;

        match parsed.command {
            JobsCommand::Submit => {
                let object = parsed.submit.as_ref().ok_or(JobsCommandError::Parse(
                    crate::jobs::wire::JobsRequestParseError::InvalidArguments,
                ))?;
                let (job_id, dispatch) = self.submit_job(
                    peer,
                    object,
                    message,
                    context.identity_provider,
                    context.security,
                    now_ns,
                )?;
                Ok(SupervisorJobsMessageResponse {
                    response: None,
                    wait: Some(JobsPendingWait::Submit { job_id }),
                    close_after: false,
                    dispatch: Some(SupervisorJobsCommandDispatch::Submit(dispatch)),
                    access_denial: None,
                    error: None,
                })
            }
            JobsCommand::Status => {
                let job_id = self.resolve_job_id(&parsed)?;
                self.check_jobs_access(peer, context.security, job_id, JobAccess::QUERY, denial)?;
                self.immediate_view_response(job_id, time, None)
            }
            JobsCommand::Wait => {
                let job_id = self.resolve_job_id(&parsed)?;
                self.check_jobs_access(peer, context.security, job_id, JobAccess::QUERY, denial)?;
                let entry = self
                    .submitted
                    .get(job_id)
                    .ok_or(JobsCommandError::UnknownJob { job_id })?;
                if parsed.wait_for == JobsWaitCondition::Ready
                    && entry.definition.readiness == JobReadiness::None
                {
                    return Err(JobsCommandError::InvalidState {
                        job_id,
                        reason: "the job has no readiness protocol to wait for",
                    });
                }
                if self.jobs_wait_satisfied(job_id, parsed.wait_for) {
                    return self.immediate_view_response(job_id, time, None);
                }
                Ok(SupervisorJobsMessageResponse {
                    response: None,
                    wait: Some(JobsPendingWait::Wait {
                        job_id,
                        condition: parsed.wait_for,
                    }),
                    close_after: false,
                    dispatch: None,
                    access_denial: None,
                    error: None,
                })
            }
            JobsCommand::Stop => {
                let job_id = self.resolve_job_id(&parsed)?;
                self.check_jobs_access(peer, context.security, job_id, JobAccess::STOP, denial)?;
                let dispatch = self.stop_submitted_job(job_id, context.controller, now_ns)?;
                if parsed.wait && !self.submitted_job_terminal(job_id) {
                    return Ok(SupervisorJobsMessageResponse {
                        response: None,
                        wait: Some(JobsPendingWait::Stop { job_id }),
                        close_after: false,
                        dispatch,
                        access_denial: None,
                        error: None,
                    });
                }
                self.immediate_view_response(job_id, time, dispatch)
            }
            JobsCommand::Signal => {
                let job_id = self.resolve_job_id(&parsed)?;
                self.check_jobs_access(peer, context.security, job_id, JobAccess::SIGNAL, denial)?;
                let signal = parsed.signal.ok_or(JobsCommandError::Parse(
                    crate::jobs::wire::JobsRequestParseError::InvalidArguments,
                ))?;
                let record = self
                    .jobs
                    .get(job_id)
                    .filter(|record| record.state == JobState::Running)
                    .cloned()
                    .ok_or(JobsCommandError::InvalidState {
                        job_id,
                        reason: "the job is not running",
                    })?;
                let target = submitted_process_target(&record)
                    .map_err(|error| JobsCommandError::Internal(format!("{error:?}")))?;
                context
                    .controller
                    .signal_main(&target, ProcessSignal::Number(signal))
                    .map_err(|error| match error {
                        crate::boundary::BoundaryError::Process(message)
                            if message.contains("unsupported signal number") =>
                        {
                            JobsCommandError::Parse(
                                crate::jobs::wire::JobsRequestParseError::InvalidArguments,
                            )
                        }
                        other => JobsCommandError::Internal(format!("{other:?}")),
                    })?;
                self.immediate_view_response(
                    job_id,
                    time,
                    Some(SupervisorJobsCommandDispatch::Signal { job_id, signal }),
                )
            }
        }
    }

    fn resolve_job_id(&self, parsed: &ParsedJobsRequest) -> Result<JobId, JobsCommandError> {
        parsed
            .job_id
            .as_deref()
            .ok_or(JobsCommandError::InvalidJobId)?
            .parse::<JobId>()
            .map_err(|_| JobsCommandError::InvalidJobId)
    }

    /// Authorise a command against the job's own descriptor. An unknown job
    /// is reported as unknown: identifiers are unguessable (§7.10).
    fn check_jobs_access<D>(
        &self,
        peer: &JobsPeer,
        checker: &mut D,
        job_id: JobId,
        desired_access: JobAccess,
        denial: &mut Option<JobAccessDenied>,
    ) -> Result<(), JobsCommandError>
    where
        D: JobAccessChecker + ?Sized,
    {
        let entry = self
            .submitted
            .get(job_id)
            .ok_or(JobsCommandError::UnknownJob { job_id })?;
        let decision = checker
            .check_job_access(JobAccessCheckRequest {
                token_fd: peer.control.token_fd(),
                descriptor: &entry.security_descriptor,
                desired_access,
            })
            .map_err(|error| JobsCommandError::Internal(format!("{error:?}")))?;
        if decision.allowed {
            return Ok(());
        }
        let denied = JobAccessDenied {
            caller: peer.control.summary.clone(),
            job_id,
            desired_access,
            granted_access_bits: decision.granted_access_bits,
        };
        *denial = Some(denied.clone());
        Err(JobsCommandError::AccessDenied(Box::new(denied)))
    }

    fn immediate_view_response(
        &self,
        job_id: JobId,
        time: ControlResponseTimeProjection,
        dispatch: Option<SupervisorJobsCommandDispatch>,
    ) -> Result<SupervisorJobsMessageResponse, JobsCommandError> {
        let frame = self.jobs_view_frame(job_id, time)?;
        Ok(SupervisorJobsMessageResponse {
            response: Some(frame),
            wait: None,
            close_after: false,
            dispatch,
            access_denial: None,
            error: None,
        })
    }

    /// The job view as a record, with the process handle attached when the
    /// job is running. A handle that cannot be duplicated is an internal
    /// error, not a silently handle-less answer: the submitter was promised
    /// one, and `status` will hand it another chance.
    pub(in crate::supervisor) fn jobs_view_frame(
        &self,
        job_id: JobId,
        time: ControlResponseTimeProjection,
    ) -> Result<JobsResponseFrame, JobsCommandError> {
        let view = self
            .submitted_job_view(job_id)
            .ok_or(JobsCommandError::UnknownJob { job_id })?;
        let bytes = jobs_job_response(job_view_json(&view, time))
            .map_err(|error| JobsCommandError::Internal(error.to_string()))?;
        let fd = self
            .jobs
            .get(job_id)
            .filter(|record| record.state == JobState::Running)
            .and_then(|record| record.pidfd)
            .map(|pidfd| {
                let borrowed = unsafe { BorrowedFd::borrow_raw(pidfd) };
                borrowed.try_clone_to_owned().map_err(|error| {
                    JobsCommandError::Internal(format!("duplicate pidfd {pidfd}: {error}"))
                })
            })
            .transpose()?;
        Ok(JobsResponseFrame { bytes, fd })
    }

    pub(in crate::supervisor) fn jobs_wait_satisfied(
        &self,
        job_id: JobId,
        condition: JobsWaitCondition,
    ) -> bool {
        let Some(entry) = self.submitted.get(job_id) else {
            return true;
        };
        if entry.outcome.is_some() {
            return true;
        }
        match condition {
            JobsWaitCondition::Terminal => false,
            JobsWaitCondition::Ready => entry.ready == Some(true),
        }
    }

    pub(in crate::supervisor) fn submitted_job_terminal(&self, job_id: JobId) -> bool {
        self.submitted
            .get(job_id)
            .is_none_or(|entry| entry.outcome.is_some())
    }

    /// An explicit stop from either socket.
    pub(in crate::supervisor) fn stop_submitted_job<P>(
        &mut self,
        job_id: JobId,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Option<SupervisorJobsCommandDispatch>, JobsCommandError>
    where
        P: ProcessController + ?Sized,
    {
        if self.submitted.get(job_id).is_none() {
            return Err(JobsCommandError::UnknownJob { job_id });
        }
        let post_kill_timeout_secs = self.settings.shutdown.post_kill_timeout_secs;
        let mut work = SupervisorWork::from_supervisor(self);
        let outcome = begin_submitted_stop(
            &mut work,
            controller,
            job_id,
            SubmittedJobCause::ExplicitStop,
            now_ns,
            post_kill_timeout_secs,
        )
        .map_err(|error| JobsCommandError::Internal(format!("{error:?}")))?;
        work.commit(self);
        Ok(match outcome {
            SubmittedStopOutcome::Unchanged => None,
            SubmittedStopOutcome::CancelledBeforeStart(failure) => {
                Some(SupervisorJobsCommandDispatch::Cancelled(*failure))
            }
            SubmittedStopOutcome::Stopping(dispatch) => {
                Some(SupervisorJobsCommandDispatch::Stop(dispatch))
            }
        })
    }
}

pub(in crate::supervisor) fn time_projection<C>(
    clock: &mut C,
    monotonic_now_ns: u64,
) -> Result<ControlResponseTimeProjection, JobsCommandError>
where
    C: RealtimeClock + ?Sized,
{
    let realtime_now_ns = clock
        .realtime_ns()
        .map_err(|error| JobsCommandError::Internal(format!("{error:?}")))?;
    Ok(ControlResponseTimeProjection::new(
        monotonic_now_ns,
        realtime_now_ns,
    ))
}

impl From<SupervisorError> for JobsCommandError {
    fn from(error: SupervisorError) -> Self {
        JobsCommandError::Internal(format!("{error:?}"))
    }
}
