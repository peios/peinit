use std::io::{self, Write};

use crate::control::client::{ControlClient, ControlClientError, ControlResponse};

use super::args::{ParseOutcome, UsageError, parse, usage_text};
use super::command::{Command, Invocation, OutputMode, ServiceAction};
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
        Ok(ParseOutcome::Run(invocation)) => run_invocation(invocation, out, err),
        Ok(ParseOutcome::Help { program }) => write_help(out, &program),
        Ok(ParseOutcome::Version) => {
            let _ = writeln!(out, "svctl {}", env!("CARGO_PKG_VERSION"));
            EXIT_OK
        }
        Err(error) => write_usage_error(err, &error),
    }
}

fn run_invocation(invocation: Invocation, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match send(&invocation) {
        Ok(response) => {
            if response.is_ok() {
                let _ = write_response(out, &invocation.command, &response, invocation.output);
                EXIT_OK
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
                ControlClientError::Io(_) => EXIT_UNAVAILABLE,
                ControlClientError::Protocol(_) => EXIT_PROTOCOL,
            };
            let _ = writeln!(err, "svctl: {error}");
            code
        }
    }
}

fn send(invocation: &Invocation) -> Result<ControlResponse, ControlClientError> {
    let mut client = ControlClient::connect_path(&invocation.socket_path)?;
    match &invocation.command {
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
    }
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
