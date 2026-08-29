//! The job view (PSPU §7.7): the one shape a submitted job is reported in,
//! on either socket.

use serde_json::{Value, json};

use crate::control::wire::ControlResponseTimeProjection;
use crate::ids::JobId;
use crate::job::{JobRecord, JobState};

use super::model::{JobProgress, JobProgressUnit, SubmittedJobCause, SubmittedJobEntry};

/// The state-dependent facts, taken from the live record while there is one
/// and from the retained outcome afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobViewState {
    pub state: JobState,
    pub pid: Option<u32>,
    pub started_at_ns: Option<u64>,
    pub ended_at_ns: Option<u64>,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobView {
    pub id: JobId,
    pub state: JobViewState,
    pub cause: Option<SubmittedJobCause>,
    pub submitter_sid: String,
    pub identity_sid: String,
    pub logon_session: u64,
    pub description: String,
    pub image_path: String,
    pub ready: Option<bool>,
    pub status_text: Option<String>,
    pub progress: Option<JobProgress>,
    pub progress_unit: Option<JobProgressUnit>,
    pub created_at_ns: u64,
}

/// Build the view from the entry and, while the job is live, its record.
/// A live entry with no record is a job whose record was dropped without
/// the outcome being recorded — an invariant violation the caller reports
/// rather than something this function guesses at.
pub fn job_view(entry: &SubmittedJobEntry, record: Option<&JobRecord>) -> Option<JobView> {
    let state = match (&entry.outcome, record) {
        (Some(outcome), _) => JobViewState {
            state: outcome.state,
            pid: match outcome.state {
                JobState::Abandoned => outcome.pid,
                _ => None,
            },
            started_at_ns: outcome.started_at_ns,
            ended_at_ns: Some(outcome.ended_at_ns),
            exit_code: outcome.exit_code,
            exit_signal: outcome.exit_signal,
        },
        (None, Some(record)) => JobViewState {
            state: record.state,
            pid: record.pid,
            started_at_ns: record.started_at_ns,
            ended_at_ns: record.ended_at_ns,
            exit_code: record.exit_code,
            exit_signal: record.exit_signal,
        },
        (None, None) => return None,
    };
    Some(JobView {
        id: entry.job_id,
        state,
        cause: entry.cause,
        submitter_sid: entry.submitter_sid.clone(),
        identity_sid: entry.identity.user_sid.clone(),
        logon_session: entry.identity.logon_session,
        description: entry.definition.description.clone(),
        image_path: entry.definition.image_path.clone(),
        ready: entry.ready,
        status_text: entry.status_text.clone(),
        progress: entry.progress,
        progress_unit: entry.progress_unit,
        created_at_ns: entry.created_at_ns,
    })
}

pub fn job_view_json(view: &JobView, time: ControlResponseTimeProjection) -> Value {
    json!({
        "id": view.id.to_canonical_string(),
        "type": "submitted",
        "state": job_state_wire(view.state.state),
        "cause": view.cause.map(SubmittedJobCause::wire),
        "submitter": view.submitter_sid.as_str(),
        "identity": view.identity_sid.as_str(),
        "logon_session": view.logon_session,
        "description": view.description.as_str(),
        "image_path": view.image_path.as_str(),
        "pid": view.state.pid,
        "ready": view.ready,
        "exit_code": view.state.exit_code,
        "exit_signal": view.state.exit_signal,
        "status_text": view.status_text.as_deref(),
        "progress": view.progress.map(|progress| json!({
            "current": progress.current,
            "total": progress.total,
            "bounded": progress.bounded,
            "unit": view.progress_unit.map(JobProgressUnit::wire),
        })),
        "created_at": time.realtime_timestamp(view.created_at_ns),
        "started_at": view.state.started_at_ns.map(|at| time.realtime_timestamp(at)),
        "ended_at": view.state.ended_at_ns.map(|at| time.realtime_timestamp(at)),
    })
}

pub fn job_state_wire(state: JobState) -> &'static str {
    match state {
        JobState::Created => "created",
        JobState::Running => "running",
        JobState::Completed => "completed",
        JobState::Failed => "failed",
        JobState::Abandoned => "abandoned",
    }
}

pub fn parse_job_state_wire(value: &str) -> Option<JobState> {
    match value {
        "created" => Some(JobState::Created),
        "running" => Some(JobState::Running),
        "completed" => Some(JobState::Completed),
        "failed" => Some(JobState::Failed),
        "abandoned" => Some(JobState::Abandoned),
        _ => None,
    }
}
