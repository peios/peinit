//! The control channel's job commands (PSPU §4.14): `job-list`,
//! `job-status`, `job-stop`, authorised against each job's descriptor.

use serde_json::json;

use crate::boundary::{Clock, ProcessController, RealtimeClock};
use crate::control::system::ControlPeer;
use crate::control::wire::{
    ControlResponseStatus, ControlResponseTimeProjection, ParsedControlRequest,
};
use crate::ids::JobId;
use crate::submitted::{
    JobAccess, JobAccessCheckRequest, JobAccessChecker, JobAccessDenied, job_view_json,
};

use super::error::JobsCommandError;
use crate::supervisor::control_command::SupervisorControlCommandBodyError;
use crate::supervisor::control_command::SupervisorControlCommandBodyResponse;
use crate::supervisor::dispatch::SupervisorJobsCommandDispatch;
use crate::supervisor::state::Supervisor;

impl Supervisor {
    pub(in crate::supervisor) fn run_control_job_status<C, A>(
        &self,
        parsed: &ParsedControlRequest,
        peer: &ControlPeer,
        access_checker: &mut A,
        clock: &mut C,
    ) -> Result<SupervisorControlCommandBodyResponse, SupervisorControlCommandBodyError>
    where
        C: Clock + RealtimeClock + ?Sized,
        A: JobAccessChecker + ?Sized,
    {
        let job_id = parse_control_job_id(parsed)?;
        self.check_control_job_access(peer, access_checker, job_id, JobAccess::QUERY)?;
        let time = control_time_projection(clock)?;
        let view = self
            .submitted_job_view(job_id)
            .ok_or(SupervisorControlCommandBodyError::UnknownJob { job_id })?;
        let line = crate::control::wire::response_line_from_value(json!({
            "status": ControlResponseStatus::Ok.as_str(),
            "job": job_view_json(&view, time),
        }))
        .map_err(SupervisorControlCommandBodyError::serialize)?;
        Ok(SupervisorControlCommandBodyResponse::accepted_response(
            line,
        ))
    }

    pub(in crate::supervisor) fn run_control_job_list<C, A>(
        &self,
        parsed: &ParsedControlRequest,
        peer: &ControlPeer,
        access_checker: &mut A,
        clock: &mut C,
    ) -> Result<SupervisorControlCommandBodyResponse, SupervisorControlCommandBodyError>
    where
        C: Clock + RealtimeClock + ?Sized,
        A: JobAccessChecker + ?Sized,
    {
        let filter = parsed.job_filter.clone().unwrap_or_default();
        let time = control_time_projection(clock)?;
        let mut jobs = Vec::new();
        let mut denials = Vec::new();
        for job_id in self
            .submitted
            .filtered_ids(&filter, |entry| self.submitted_job_state(entry))
        {
            match self.control_job_access_decision(
                peer,
                access_checker,
                job_id,
                JobAccess::QUERY,
            )? {
                Ok(()) => {
                    if let Some(view) = self.submitted_job_view(job_id) {
                        jobs.push(job_view_json(&view, time));
                    }
                }
                Err(denied) => denials.push(denied),
            }
        }
        let line = crate::control::wire::response_line_from_value(json!({
            "status": ControlResponseStatus::Ok.as_str(),
            "jobs": jobs,
        }))
        .map_err(SupervisorControlCommandBodyError::serialize)?;
        Ok(SupervisorControlCommandBodyResponse::Accepted {
            response_line: Some(line),
            dispatch: None,
            wait: None,
            access_denials: Vec::new(),
            job_access_denials: denials,
        })
    }

    pub(in crate::supervisor) fn run_control_job_stop<C, P, A>(
        &mut self,
        parsed: &ParsedControlRequest,
        peer: &ControlPeer,
        access_checker: &mut A,
        controller: &mut P,
        clock: &mut C,
    ) -> Result<SupervisorControlCommandBodyResponse, SupervisorControlCommandBodyError>
    where
        C: Clock + RealtimeClock + ?Sized,
        P: ProcessController + ?Sized,
        A: JobAccessChecker + ?Sized,
    {
        let job_id = parse_control_job_id(parsed)?;
        self.check_control_job_access(peer, access_checker, job_id, JobAccess::STOP)?;
        let now_ns = clock.monotonic_ns().map_err(|error| {
            SupervisorControlCommandBodyError::supervisor(
                crate::supervisor::SupervisorError::Clock(error),
            )
        })?;
        let dispatch = self
            .stop_submitted_job(job_id, controller, now_ns)
            .map_err(jobs_error_to_control)?;
        let dispatch = dispatch.map(|dispatch| {
            Box::new(crate::supervisor::SupervisorControlCommandDispatch::Job(
                Box::new(dispatch),
            ))
        });
        if parsed.wait && !self.submitted_job_terminal(job_id) {
            return Ok(SupervisorControlCommandBodyResponse::Accepted {
                response_line: None,
                dispatch,
                wait: Some(crate::control::connection::ControlPendingWait::Job { job_id }),
                access_denials: Vec::new(),
                job_access_denials: Vec::new(),
            });
        }
        let time = control_time_projection(clock)?;
        let line = self
            .control_job_view_line(job_id, time)
            .map_err(SupervisorControlCommandBodyError::serialize)?;
        Ok(SupervisorControlCommandBodyResponse::Accepted {
            response_line: line,
            dispatch,
            wait: None,
            access_denials: Vec::new(),
            job_access_denials: Vec::new(),
        })
    }

    /// The `job-stop` wait's answer: the view, or `UNKNOWN_JOB` once the
    /// record is gone.
    pub(in crate::supervisor) fn control_job_view_line(
        &self,
        job_id: JobId,
        time: ControlResponseTimeProjection,
    ) -> Result<Option<Vec<u8>>, serde_json::Error> {
        match self.submitted_job_view(job_id) {
            Some(view) => crate::control::wire::response_line_from_value(json!({
                "status": ControlResponseStatus::Ok.as_str(),
                "job": job_view_json(&view, time),
            }))
            .map(Some),
            None => crate::control::wire::control_error_response_line(
                crate::control::wire::ControlErrorCode::UnknownJob,
                &format!("unknown job {job_id}"),
            )
            .map(Some),
        }
    }

    fn check_control_job_access<A>(
        &self,
        peer: &ControlPeer,
        access_checker: &mut A,
        job_id: JobId,
        desired_access: JobAccess,
    ) -> Result<(), SupervisorControlCommandBodyError>
    where
        A: JobAccessChecker + ?Sized,
    {
        match self.control_job_access_decision(peer, access_checker, job_id, desired_access)? {
            Ok(()) => Ok(()),
            Err(denied) => Err(SupervisorControlCommandBodyError::JobAccessDenied(
                Box::new(denied),
            )),
        }
    }

    fn control_job_access_decision<A>(
        &self,
        peer: &ControlPeer,
        access_checker: &mut A,
        job_id: JobId,
        desired_access: JobAccess,
    ) -> Result<Result<(), JobAccessDenied>, SupervisorControlCommandBodyError>
    where
        A: JobAccessChecker + ?Sized,
    {
        let entry = self
            .submitted
            .get(job_id)
            .ok_or(SupervisorControlCommandBodyError::UnknownJob { job_id })?;
        let decision = access_checker
            .check_job_access(JobAccessCheckRequest {
                token_fd: peer.token_fd(),
                descriptor: &entry.security_descriptor,
                desired_access,
            })
            .map_err(SupervisorControlCommandBodyError::JobAuthorization)?;
        if decision.allowed {
            Ok(Ok(()))
        } else {
            Ok(Err(JobAccessDenied {
                caller: peer.summary.clone(),
                job_id,
                desired_access,
                granted_access_bits: decision.granted_access_bits,
            }))
        }
    }
}

fn parse_control_job_id(
    parsed: &ParsedControlRequest,
) -> Result<JobId, SupervisorControlCommandBodyError> {
    parsed
        .job_id
        .as_deref()
        .ok_or(SupervisorControlCommandBodyError::InvalidArguments)?
        .parse::<JobId>()
        .map_err(|_| SupervisorControlCommandBodyError::InvalidArguments)
}

fn control_time_projection<C>(
    clock: &mut C,
) -> Result<ControlResponseTimeProjection, SupervisorControlCommandBodyError>
where
    C: Clock + RealtimeClock + ?Sized,
{
    let monotonic_now_ns = clock.monotonic_ns().map_err(|error| {
        SupervisorControlCommandBodyError::supervisor(crate::supervisor::SupervisorError::Clock(
            error,
        ))
    })?;
    let realtime_now_ns = clock.realtime_ns().map_err(|error| {
        SupervisorControlCommandBodyError::supervisor(crate::supervisor::SupervisorError::Clock(
            error,
        ))
    })?;
    Ok(ControlResponseTimeProjection::new(
        monotonic_now_ns,
        realtime_now_ns,
    ))
}

fn jobs_error_to_control(error: JobsCommandError) -> SupervisorControlCommandBodyError {
    match error {
        JobsCommandError::UnknownJob { job_id } => {
            SupervisorControlCommandBodyError::UnknownJob { job_id }
        }
        other => SupervisorControlCommandBodyError::ResponseSerialize(format!("{other:?}")),
    }
}

impl SupervisorJobsCommandDispatch {
    pub fn job_id(&self) -> Option<JobId> {
        match self {
            Self::Submit(dispatch) => Some(dispatch.job_event.job_id),
            Self::Stop(dispatch) => Some(dispatch.job_id),
            Self::Cancelled(dispatch) => Some(dispatch.job_event.job_id),
            Self::Signal { job_id, .. } => Some(*job_id),
        }
    }
}
