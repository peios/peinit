use std::io::{self, Write};
use std::os::fd::BorrowedFd;

use serde_json::{Value, json};

use crate::control::client::{ControlClient, ControlClientError, ControlResponse};
use crate::jobs::client::{JobsClient, JobsClientError, JobsResponse};

use super::args::{ParseOutcome, UsageError, parse, usage_text};
use super::command::{
    Channel, Command, Invocation, JobListFilter, JobSubmission, OutputMode, ServiceAction,
};
use super::output::{write_response, write_server_error};

const EXIT_OK: i32 = 0;
const EXIT_SERVER_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 64;
const EXIT_UNAVAILABLE: i32 = 69;
const EXIT_PROTOCOL: i32 = 70;

pub fn run_from_env() -> i32 {
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut stdout = stdout.lock();
    let mut stderr = stderr.lock();
    run_with_io(std::env::args_os(), &mut stdout, &mut stderr)
}

pub fn run_with_io<I, S>(args: I, out: &mut dyn Write, err: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString>,
{
    match parse(args) {
        Ok(ParseOutcome::Run(invocation)) => run_invocation(*invocation, out, err),
        Ok(ParseOutcome::Help { program }) => write_help(out, &program),
        Ok(ParseOutcome::Version) => {
            let _ = writeln!(out, "svctl {}", env!("CARGO_PKG_VERSION"));
            EXIT_OK
        }
        Err(error) => write_usage_error(err, &error),
    }
}

/// One answer from either door, as the output layer sees it.
#[derive(Debug)]
pub struct CliResponse {
    raw_json: String,
    value: Value,
    ok: bool,
    error_code: Option<String>,
    error_message: Option<String>,
}

impl CliResponse {
    pub fn raw_json(&self) -> &str {
        &self.raw_json
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn is_ok(&self) -> bool {
        self.ok
    }

    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }
}

impl From<ControlResponse> for CliResponse {
    fn from(response: ControlResponse) -> Self {
        Self {
            ok: response.is_ok(),
            error_code: response.error_code().map(str::to_string),
            error_message: response.error_message().map(str::to_string),
            raw_json: response.raw_json().to_string(),
            value: response.value().clone(),
        }
    }
}

impl From<JobsResponse> for CliResponse {
    fn from(response: JobsResponse) -> Self {
        Self {
            ok: response.ok,
            error_code: response.error_code.map(|code| code.as_str().to_string()),
            error_message: response.error_message,
            raw_json: response.raw_json,
            value: response.value,
        }
    }
}

/// Why a request could not be made at all, as distinct from a server refusal.
#[derive(Debug)]
enum SendError {
    Io(String),
    Protocol(String),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) | Self::Protocol(message) => f.write_str(message),
        }
    }
}

impl From<ControlClientError> for SendError {
    fn from(error: ControlClientError) -> Self {
        match error {
            ControlClientError::Io(message) => Self::Io(message),
            ControlClientError::Protocol(message) => Self::Protocol(message),
        }
    }
}

impl From<JobsClientError> for SendError {
    fn from(error: JobsClientError) -> Self {
        match error {
            JobsClientError::Io(message) => Self::Io(message),
            JobsClientError::Protocol(message) => Self::Protocol(message),
        }
    }
}

fn run_invocation(invocation: Invocation, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match send(&invocation) {
        Ok(response) => {
            if response.is_ok() {
                let _ = write_response(out, &invocation.command, &response, invocation.output);
                if job_did_not_complete(&invocation.command, &response) {
                    let _ = writeln!(err, "svctl: the job did not complete");
                    EXIT_SERVER_ERROR
                } else {
                    EXIT_OK
                }
            } else {
                if invocation.output == OutputMode::Json {
                    let _ = write_response(out, &invocation.command, &response, invocation.output);
                }
                let _ = write_server_error(err, &response, invocation.output);
                EXIT_SERVER_ERROR
            }
        }
        Err(error) => {
            let code = match error {
                SendError::Io(_) => EXIT_UNAVAILABLE,
                SendError::Protocol(_) => EXIT_PROTOCOL,
            };
            let _ = writeln!(err, "svctl: {error}");
            code
        }
    }
}

/// `job submit --wait` is a run: its exit status follows the job's.
fn job_did_not_complete(command: &Command, response: &CliResponse) -> bool {
    matches!(command, Command::JobSubmit { wait: true, .. })
        && response
            .value()
            .get("job")
            .and_then(|job| job.get("state"))
            .and_then(Value::as_str)
            != Some("completed")
}

fn send(invocation: &Invocation) -> Result<CliResponse, SendError> {
    match invocation.command.channel() {
        Channel::Control => {
            let mut client = ControlClient::connect_path(&invocation.socket_path)?;
            send_control(&mut client, &invocation.command)
        }
        Channel::Jobs => {
            let mut client = JobsClient::connect_path(&invocation.jobs_socket_path)?;
            send_jobs(&mut client, &invocation.command)
        }
    }
}

fn send_control(client: &mut ControlClient, command: &Command) -> Result<CliResponse, SendError> {
    let response = match command {
        Command::Service {
            action,
            service,
            wait,
        } => match action {
            ServiceAction::Start => client.service_start(service, *wait),
            ServiceAction::Stop => client.service_stop(service, *wait),
            ServiceAction::Restart => client.service_restart(service, *wait),
            ServiceAction::Reload => client.service_reload(service, *wait),
            ServiceAction::Reset => client.service_reset(service),
        },
        Command::Status { service } => client.service_status(service),
        Command::List => client.service_list(),
        Command::OperationStatus { operation_id } => client.operation_status(operation_id),
        Command::ReloadConfig => client.reload_config(),
        Command::Shutdown { kind } => client.system_shutdown(*kind),
        Command::JobList { filter } => client.request(job_list_request(filter)),
        Command::JobStatus { job_id } => {
            client.request(json!({"command": "job-status", "job_id": job_id}))
        }
        Command::JobStop { job_id, wait } => {
            client.request(json!({"command": "job-stop", "job_id": job_id, "wait": wait}))
        }
        Command::JobSubmit { .. } | Command::JobWait { .. } | Command::JobSignal { .. } => {
            unreachable!("jobs-channel command routed to the control socket")
        }
    }?;
    Ok(response.into())
}

fn job_list_request(filter: &JobListFilter) -> Value {
    let mut request = json!({"command": "job-list"});
    if let Some(submitter) = &filter.submitter {
        request["submitter"] = json!(submitter);
    }
    if let Some(identity) = &filter.identity {
        request["identity"] = json!(identity);
    }
    if let Some(logon_session) = filter.logon_session {
        request["logon_session"] = json!(logon_session);
    }
    if let Some(state) = &filter.state {
        request["state"] = json!(state);
    }
    request
}

fn send_jobs(client: &mut JobsClient, command: &Command) -> Result<CliResponse, SendError> {
    let response = match command {
        Command::JobSubmit { submission, wait } => {
            let response = submit(client, submission)?;
            if !*wait || !response.ok {
                return Ok(response.into());
            }
            let job_id = response
                .job()
                .and_then(|job| job.get("id"))
                .and_then(Value::as_str)
                .ok_or_else(|| SendError::Protocol("submit response names no job".to_string()))?
                .to_string();
            // The job is running (or already over): wait on the same
            // connection, so the answer is the job's terminal view.
            client.wait(&job_id, false)?
        }
        Command::JobWait { job_id, for_ready } => client.wait(job_id, *for_ready)?,
        Command::JobSignal { job_id, signal } => client.signal(job_id, *signal)?,
        _ => unreachable!("control-socket command routed to the jobs socket"),
    };
    Ok(response.into())
}

fn submit(client: &mut JobsClient, submission: &JobSubmission) -> Result<JobsResponse, SendError> {
    let definition = submission_definition(submission);
    // The descriptors travel in the order the definition names them, then
    // the output sink last (PSPU §7.9). They are this process's own.
    let mut fds: Vec<BorrowedFd<'_>> = submission
        .descriptors
        .iter()
        .map(|(_, fd)| unsafe { BorrowedFd::borrow_raw(*fd) })
        .collect();
    if submission.output {
        fds.push(unsafe { BorrowedFd::borrow_raw(libc::STDOUT_FILENO) });
    }
    Ok(client.submit(definition, None, &fds)?)
}

pub(super) fn submission_definition(submission: &JobSubmission) -> Value {
    let mut definition = json!({
        "image_path": submission.image_path,
        "arguments": submission.arguments,
    });
    if !submission.environment.is_empty() {
        definition["environment"] = submission
            .environment
            .iter()
            .map(|(name, value)| (name.clone(), json!(value)))
            .collect::<serde_json::Map<String, Value>>()
            .into();
    }
    if let Some(directory) = &submission.working_directory {
        definition["working_directory"] = json!(directory);
    }
    if let Some(description) = &submission.description {
        definition["description"] = json!(description);
    }
    if let Some(timeout) = submission.timeout_secs {
        definition["timeout"] = json!(timeout);
    }
    if let Some(timeout) = submission.stop_timeout_secs {
        definition["stop_timeout"] = json!(timeout);
    }
    if let Some(readiness) = &submission.readiness {
        definition["readiness"] = json!(readiness);
    }
    if let Some(timeout) = submission.readiness_timeout_secs {
        definition["readiness_timeout"] = json!(timeout);
    }
    if let Some(codes) = &submission.success_exit_codes {
        definition["success_exit_codes"] = json!(codes);
    }
    if !submission.descriptors.is_empty() {
        definition["descriptors"] = submission
            .descriptors
            .iter()
            .map(|(name, _)| json!(name))
            .collect::<Vec<_>>()
            .into();
    }
    if submission.output {
        definition["output"] = json!(true);
    }
    if let Some(sddl) = &submission.security_descriptor {
        definition["security_descriptor"] = json!(sddl);
    }
    definition
}

fn write_help(out: &mut dyn Write, program: &str) -> i32 {
    let _ = write!(out, "{}", usage_text(program));
    EXIT_OK
}

fn write_usage_error(err: &mut dyn Write, error: &UsageError) -> i32 {
    let _ = writeln!(err, "svctl: {}", error.message);
    let _ = writeln!(err);
    let _ = write!(err, "{}", usage_text(&error.program));
    EXIT_USAGE
}
