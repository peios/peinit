use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::control::socket::CONTROL_SOCKET_PATH;
use crate::jobs::socket::JOBS_SOCKET_PATH;
use crate::shutdown::ShutdownKind;

use super::command::{
    Command, Invocation, JobListFilter, JobSubmission, OutputMode, ServiceAction,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    Run(Box<Invocation>),
    Help { program: String },
    Version,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    pub program: String,
    pub message: String,
}

#[derive(Debug, Default)]
struct GlobalArgs {
    socket_path: Option<PathBuf>,
    jobs_socket_path: Option<PathBuf>,
    output: Option<OutputMode>,
    wait: Option<bool>,
    positionals: Vec<String>,
}

pub fn parse<I, S>(args: I) -> Result<ParseOutcome, UsageError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let program_os = args.next().unwrap_or_else(|| OsString::from("svctl"));
    let program = display_os(&program_os);
    let mode = program_mode(&program);
    let mut global = GlobalArgs::default();

    while let Some(arg_os) = args.next() {
        if arg_os == "--socket" {
            let Some(path) = args.next() else {
                return Err(usage(&program, "--socket requires a path"));
            };
            global.socket_path = Some(PathBuf::from(path));
            continue;
        }
        if arg_os == "--jobs-socket" {
            let Some(path) = args.next() else {
                return Err(usage(&program, "--jobs-socket requires a path"));
            };
            global.jobs_socket_path = Some(PathBuf::from(path));
            continue;
        }

        let arg = string_arg(&program, arg_os)?;
        if let Some(path) = arg.strip_prefix("--socket=") {
            if path.is_empty() {
                return Err(usage(&program, "--socket requires a path"));
            }
            global.socket_path = Some(PathBuf::from(path));
        } else if let Some(path) = arg.strip_prefix("--jobs-socket=") {
            if path.is_empty() {
                return Err(usage(&program, "--jobs-socket requires a path"));
            }
            global.jobs_socket_path = Some(PathBuf::from(path));
        } else if arg == "--json" {
            global.output = Some(OutputMode::Json);
        } else if arg == "--wait" {
            global.wait = Some(true);
        } else if arg == "--no-wait" {
            global.wait = Some(false);
        } else if arg == "-h" || arg == "--help" {
            return Ok(ParseOutcome::Help { program });
        } else if arg == "--version" {
            return Ok(ParseOutcome::Version);
        } else {
            global.positionals.push(arg);
        }
    }

    let command = match mode {
        ProgramMode::Svctl => parse_svctl_command(&program, &global.positionals, global.wait)?,
        ProgramMode::Reboot => parse_multicall_shutdown(&program, &global, ShutdownKind::Reboot)?,
        ProgramMode::Poweroff => {
            parse_multicall_shutdown(&program, &global, ShutdownKind::Poweroff)?
        }
        ProgramMode::Halt => parse_multicall_shutdown(&program, &global, ShutdownKind::Halt)?,
        ProgramMode::Shutdown => parse_classic_shutdown(&program, &global)?,
    };
    let socket_path = global
        .socket_path
        .unwrap_or_else(|| PathBuf::from(CONTROL_SOCKET_PATH));
    let jobs_socket_path = global
        .jobs_socket_path
        .unwrap_or_else(|| PathBuf::from(JOBS_SOCKET_PATH));
    let output = global.output.unwrap_or(OutputMode::Human);

    Ok(ParseOutcome::Run(Box::new(Invocation {
        program,
        socket_path,
        jobs_socket_path,
        output,
        command,
    })))
}

fn parse_svctl_command(
    program: &str,
    positionals: &[String],
    wait: Option<bool>,
) -> Result<Command, UsageError> {
    let Some(command) = positionals.first() else {
        return Err(usage(program, "missing command"));
    };
    let rest = &positionals[1..];
    match command.as_str() {
        "start" => parse_service_command(program, rest, wait, ServiceAction::Start),
        "stop" => parse_service_command(program, rest, wait, ServiceAction::Stop),
        "restart" => parse_service_command(program, rest, wait, ServiceAction::Restart),
        "reload" => parse_service_command(program, rest, wait, ServiceAction::Reload),
        "reset" => parse_service_command(program, rest, wait, ServiceAction::Reset),
        "status" => {
            reject_wait(program, wait, "status")?;
            let service = single_argument(program, rest, "status requires a service")?;
            Ok(Command::Status { service })
        }
        "list" => {
            reject_wait(program, wait, "list")?;
            no_arguments(program, rest, "list does not take arguments")?;
            Ok(Command::List)
        }
        "op" | "operation-status" => {
            reject_wait(program, wait, "operation-status")?;
            let operation_id =
                single_argument(program, rest, "operation-status requires an operation id")?;
            Ok(Command::OperationStatus { operation_id })
        }
        "reload-config" => {
            reject_wait(program, wait, "reload-config")?;
            no_arguments(program, rest, "reload-config does not take arguments")?;
            Ok(Command::ReloadConfig)
        }
        "shutdown" => {
            reject_wait(program, wait, "shutdown")?;
            let kind =
                single_argument(program, rest, "shutdown requires poweroff, reboot, or halt")?;
            let kind = parse_shutdown_kind(program, &kind)?;
            Ok(Command::Shutdown { kind })
        }
        "job" => parse_job_command(program, rest, wait),
        "help" => Err(usage(program, "use --help to show usage")),
        other if other.starts_with('-') => Err(usage(program, format!("unknown option {other}"))),
        other => Err(usage(program, format!("unknown command {other}"))),
    }
}

fn parse_job_command(
    program: &str,
    positionals: &[String],
    wait: Option<bool>,
) -> Result<Command, UsageError> {
    let Some(command) = positionals.first() else {
        return Err(usage(
            program,
            "job requires a subcommand: list, status, stop, submit, wait, signal",
        ));
    };
    let rest = &positionals[1..];
    match command.as_str() {
        "list" => {
            reject_wait(program, wait, "job list")?;
            Ok(Command::JobList {
                filter: parse_job_list_filter(program, rest)?,
            })
        }
        "status" => {
            reject_wait(program, wait, "job status")?;
            let job_id = single_argument(program, rest, "job status requires a job id")?;
            Ok(Command::JobStatus { job_id })
        }
        "stop" => {
            let job_id = single_argument(program, rest, "job stop requires a job id")?;
            Ok(Command::JobStop {
                job_id,
                wait: wait.unwrap_or(true),
            })
        }
        "submit" => {
            let submission = parse_job_submission(program, rest)?;
            Ok(Command::JobSubmit {
                submission,
                wait: wait.unwrap_or(false),
            })
        }
        "wait" => {
            reject_wait(program, wait, "job wait")?;
            parse_job_wait(program, rest)
        }
        "signal" => {
            reject_wait(program, wait, "job signal")?;
            parse_job_signal(program, rest)
        }
        other if other.starts_with('-') => Err(usage(program, format!("unknown option {other}"))),
        other => Err(usage(program, format!("unknown job subcommand {other}"))),
    }
}

/// `--name value` or `--name=value`, consumed from `args`.
fn take_option_value<'a>(
    program: &str,
    args: &mut std::slice::Iter<'a, String>,
    arg: &'a str,
    name: &str,
) -> Result<Option<&'a str>, UsageError> {
    if arg == name {
        return args
            .next()
            .map(|value| Some(value.as_str()))
            .ok_or_else(|| usage(program, format!("{name} requires a value")));
    }
    match arg
        .strip_prefix(name)
        .and_then(|rest| rest.strip_prefix('='))
    {
        Some("") => Err(usage(program, format!("{name} requires a value"))),
        Some(value) => Ok(Some(value)),
        None => Ok(None),
    }
}

fn parse_job_list_filter(program: &str, args: &[String]) -> Result<JobListFilter, UsageError> {
    let mut filter = JobListFilter::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = take_option_value(program, &mut iter, arg, "--submitter")? {
            filter.submitter = Some(value.to_string());
        } else if let Some(value) = take_option_value(program, &mut iter, arg, "--identity")? {
            filter.identity = Some(value.to_string());
        } else if let Some(value) = take_option_value(program, &mut iter, arg, "--logon-session")? {
            filter.logon_session = Some(parse_u64(program, "--logon-session", value)?);
        } else if let Some(value) = take_option_value(program, &mut iter, arg, "--state")? {
            match value {
                "created" | "running" | "completed" | "failed" | "abandoned" => {
                    filter.state = Some(value.to_string());
                }
                _ => {
                    return Err(usage(
                        program,
                        "--state must be created, running, completed, failed, or abandoned",
                    ));
                }
            }
        } else if arg.starts_with('-') {
            return Err(usage(program, format!("unknown option {arg}")));
        } else {
            return Err(usage(program, "job list does not take arguments"));
        }
    }
    Ok(filter)
}

fn parse_job_submission(program: &str, args: &[String]) -> Result<JobSubmission, UsageError> {
    let mut submission = JobSubmission::default();
    let mut iter = args.iter();
    let mut image: Option<String> = None;
    while let Some(arg) = iter.next() {
        if image.is_some() {
            // Everything after the image is the job's, options included.
            submission.arguments.push(arg.clone());
        } else if arg == "--" {
            let Some(next) = iter.next() else {
                return Err(usage(program, "job submit requires an image path"));
            };
            image = Some(next.clone());
        } else if let Some(value) = take_option_value(program, &mut iter, arg, "--description")? {
            submission.description = Some(value.to_string());
        } else if let Some(value) = take_option_value(program, &mut iter, arg, "--cwd")? {
            submission.working_directory = Some(value.to_string());
        } else if let Some(value) = take_option_value(program, &mut iter, arg, "--env")? {
            let Some((name, val)) = value.split_once('=') else {
                return Err(usage(program, "--env takes NAME=VALUE"));
            };
            if name.is_empty() {
                return Err(usage(program, "--env takes NAME=VALUE"));
            }
            submission
                .environment
                .push((name.to_string(), val.to_string()));
        } else if let Some(value) = take_option_value(program, &mut iter, arg, "--timeout")? {
            submission.timeout_secs = Some(parse_u64(program, "--timeout", value)?);
        } else if let Some(value) = take_option_value(program, &mut iter, arg, "--stop-timeout")? {
            submission.stop_timeout_secs = Some(parse_u64(program, "--stop-timeout", value)?);
        } else if let Some(value) = take_option_value(program, &mut iter, arg, "--readiness")? {
            match value {
                "none" | "notify" => submission.readiness = Some(value.to_string()),
                _ => return Err(usage(program, "--readiness must be none or notify")),
            }
        } else if let Some(value) =
            take_option_value(program, &mut iter, arg, "--readiness-timeout")?
        {
            submission.readiness_timeout_secs =
                Some(parse_u64(program, "--readiness-timeout", value)?);
        } else if let Some(value) =
            take_option_value(program, &mut iter, arg, "--success-exit-code")?
        {
            let code = value
                .parse::<i32>()
                .ok()
                .filter(|code| (0..=255).contains(code))
                .ok_or_else(|| usage(program, "--success-exit-code takes 0..=255"))?;
            submission
                .success_exit_codes
                .get_or_insert_with(Vec::new)
                .push(code);
        } else if let Some(value) = take_option_value(program, &mut iter, arg, "--fd")? {
            let Some((name, fd)) = value.split_once('=') else {
                return Err(usage(program, "--fd takes NAME=FD"));
            };
            let fd = fd
                .parse::<i32>()
                .ok()
                .filter(|fd| *fd >= 0)
                .ok_or_else(|| usage(program, "--fd takes NAME=FD with a non-negative FD"))?;
            if name.is_empty() {
                return Err(usage(program, "--fd takes NAME=FD"));
            }
            submission.descriptors.push((name.to_string(), fd));
        } else if arg == "--output" {
            submission.output = true;
        } else if let Some(value) =
            take_option_value(program, &mut iter, arg, "--security-descriptor")?
        {
            submission.security_descriptor = Some(value.to_string());
        } else if arg.starts_with('-') {
            return Err(usage(program, format!("unknown option {arg}")));
        } else {
            image = Some(arg.clone());
        }
    }
    let Some(image_path) = image else {
        return Err(usage(program, "job submit requires an image path"));
    };
    if !image_path.starts_with('/') {
        return Err(usage(program, "the image path must be absolute"));
    }
    submission.image_path = image_path;
    Ok(submission)
}

fn parse_job_wait(program: &str, args: &[String]) -> Result<Command, UsageError> {
    let mut job_id: Option<String> = None;
    let mut for_ready = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = take_option_value(program, &mut iter, arg, "--for")? {
            for_ready = match value {
                "ready" => true,
                "terminal" => false,
                _ => return Err(usage(program, "--for must be ready or terminal")),
            };
        } else if arg.starts_with('-') {
            return Err(usage(program, format!("unknown option {arg}")));
        } else if job_id.is_some() {
            return Err(usage(program, "too many arguments"));
        } else {
            job_id = Some(arg.clone());
        }
    }
    let job_id = job_id.ok_or_else(|| usage(program, "job wait requires a job id"))?;
    Ok(Command::JobWait { job_id, for_ready })
}

fn parse_job_signal(program: &str, args: &[String]) -> Result<Command, UsageError> {
    match args {
        [job_id, signal] if !job_id.starts_with('-') => {
            let signal = parse_signal(signal)
                .ok_or_else(|| usage(program, "job signal takes a signal number or name"))?;
            Ok(Command::JobSignal {
                job_id: job_id.clone(),
                signal,
            })
        }
        [value, ..] if value.starts_with('-') => {
            Err(usage(program, format!("unknown option {value}")))
        }
        [] | [_] => Err(usage(program, "job signal requires a job id and a signal")),
        _ => Err(usage(program, "too many arguments")),
    }
}

/// A signal as a number, or one of the names a shell user reaches for.
fn parse_signal(value: &str) -> Option<i32> {
    if let Ok(number) = value.parse::<i32>() {
        return (number > 0).then_some(number);
    }
    let name = value.strip_prefix("SIG").unwrap_or(value);
    match name.to_ascii_uppercase().as_str() {
        "HUP" => Some(libc::SIGHUP),
        "INT" => Some(libc::SIGINT),
        "QUIT" => Some(libc::SIGQUIT),
        "KILL" => Some(libc::SIGKILL),
        "USR1" => Some(libc::SIGUSR1),
        "USR2" => Some(libc::SIGUSR2),
        "TERM" => Some(libc::SIGTERM),
        "CONT" => Some(libc::SIGCONT),
        "STOP" => Some(libc::SIGSTOP),
        _ => None,
    }
}

fn parse_u64(program: &str, name: &str, value: &str) -> Result<u64, UsageError> {
    value
        .parse::<u64>()
        .map_err(|_| usage(program, format!("{name} takes an unsigned integer")))
}

fn parse_service_command(
    program: &str,
    args: &[String],
    wait: Option<bool>,
    action: ServiceAction,
) -> Result<Command, UsageError> {
    if !action.accepts_wait() {
        reject_wait(program, wait, action.command_name())?;
    }
    let service = single_argument(
        program,
        args,
        format!("{} requires a service", action.command_name()),
    )?;
    Ok(Command::Service {
        action,
        service,
        wait: wait.unwrap_or_else(|| action.default_wait()),
    })
}

fn parse_multicall_shutdown(
    program: &str,
    global: &GlobalArgs,
    kind: ShutdownKind,
) -> Result<Command, UsageError> {
    reject_wait(program, global.wait, program)?;
    no_arguments(
        program,
        &global.positionals,
        format!("{program} does not take arguments"),
    )?;
    Ok(Command::Shutdown { kind })
}

fn parse_classic_shutdown(program: &str, global: &GlobalArgs) -> Result<Command, UsageError> {
    reject_wait(program, global.wait, "shutdown")?;
    let args = global.positionals.as_slice();
    let kind = match args {
        [time] if time == "now" => ShutdownKind::Poweroff,
        [flag, time] if time == "now" => match flag.as_str() {
            "-h" | "-P" | "--poweroff" => ShutdownKind::Poweroff,
            "-r" | "--reboot" => ShutdownKind::Reboot,
            "-H" | "--halt" => ShutdownKind::Halt,
            _ => {
                return Err(usage(
                    program,
                    format!("unsupported shutdown option {flag}"),
                ));
            }
        },
        [] => return Err(usage(program, "shutdown requires an immediate time: now")),
        _ => {
            return Err(usage(
                program,
                "only immediate shutdown forms are supported: now, -h now, -P now, -r now, -H now",
            ));
        }
    };
    Ok(Command::Shutdown { kind })
}

fn single_argument(
    program: &str,
    args: &[String],
    missing: impl Into<String>,
) -> Result<String, UsageError> {
    match args {
        [value] if !value.starts_with('-') => Ok(value.clone()),
        [] => Err(usage(program, missing)),
        [value] if value.starts_with('-') => Err(usage(program, format!("unknown option {value}"))),
        _ => Err(usage(program, "too many arguments")),
    }
}

fn no_arguments(
    program: &str,
    args: &[String],
    message: impl Into<String>,
) -> Result<(), UsageError> {
    if args.is_empty() {
        Ok(())
    } else if args[0].starts_with('-') {
        Err(usage(program, format!("unknown option {}", args[0])))
    } else {
        Err(usage(program, message))
    }
}

fn reject_wait(program: &str, wait: Option<bool>, command: &str) -> Result<(), UsageError> {
    if wait.is_some() {
        Err(usage(
            program,
            format!("--wait and --no-wait are not valid for {command}"),
        ))
    } else {
        Ok(())
    }
}

fn parse_shutdown_kind(program: &str, value: &str) -> Result<ShutdownKind, UsageError> {
    match value {
        "poweroff" => Ok(ShutdownKind::Poweroff),
        "reboot" => Ok(ShutdownKind::Reboot),
        "halt" => Ok(ShutdownKind::Halt),
        _ => Err(usage(
            program,
            "shutdown kind must be poweroff, reboot, or halt",
        )),
    }
}

fn program_mode(program: &str) -> ProgramMode {
    match Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
    {
        Some("reboot") => ProgramMode::Reboot,
        Some("poweroff") => ProgramMode::Poweroff,
        Some("halt") => ProgramMode::Halt,
        Some("shutdown") => ProgramMode::Shutdown,
        _ => ProgramMode::Svctl,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgramMode {
    Svctl,
    Reboot,
    Poweroff,
    Halt,
    Shutdown,
}

fn string_arg(program: &str, value: OsString) -> Result<String, UsageError> {
    value
        .into_string()
        .map_err(|_| usage(program, "arguments must be valid UTF-8"))
}

fn display_os(value: &OsString) -> String {
    value.to_string_lossy().into_owned()
}

fn usage(program: &str, message: impl Into<String>) -> UsageError {
    UsageError {
        program: program.to_string(),
        message: message.into(),
    }
}

pub fn usage_text(program: &str) -> String {
    format!(
        "\
Usage:
  {program} [--socket PATH] [--json] [--wait|--no-wait] start SERVICE
  {program} [--socket PATH] [--json] [--wait|--no-wait] stop SERVICE
  {program} [--socket PATH] [--json] [--wait|--no-wait] restart SERVICE
  {program} [--socket PATH] [--json] [--wait|--no-wait] reload SERVICE
  {program} [--socket PATH] [--json] reset SERVICE
  {program} [--socket PATH] [--json] status SERVICE
  {program} [--socket PATH] [--json] list
  {program} [--socket PATH] [--json] op OPERATION_ID
  {program} [--socket PATH] [--json] reload-config
  {program} [--socket PATH] [--json] shutdown poweroff|reboot|halt

Submitted jobs (list, status and stop use the control socket; submit, wait
and signal use the jobs socket):
  {program} [--socket PATH] [--json] job list [--submitter SID] [--identity SID]
                                       [--logon-session N] [--state STATE]
  {program} [--socket PATH] [--json] job status JOB_ID
  {program} [--socket PATH] [--json] [--wait|--no-wait] job stop JOB_ID
  {program} [--jobs-socket PATH] [--json] [--wait] job submit [OPTIONS] IMAGE [ARG...]
  {program} [--jobs-socket PATH] [--json] job wait [--for ready|terminal] JOB_ID
  {program} [--jobs-socket PATH] [--json] job signal JOB_ID SIGNAL

job submit options:
  --description TEXT        --cwd DIR                 --env NAME=VALUE (repeatable)
  --timeout SECS            --stop-timeout SECS       --readiness none|notify
  --readiness-timeout SECS  --success-exit-code N (repeatable)
  --fd NAME=FD (repeatable) --output                  --security-descriptor SDDL
The job runs as this process's own primary token. --fd passes a descriptor of
this process to the job under NAME; --output attaches standard output as the
job's output sink; --wait waits for the job to end and exits 0 only if it
completed.

Immediate shutdown multicall forms are also supported:
  reboot
  poweroff
  halt
  shutdown now
  shutdown -h now
  shutdown -P now
  shutdown -r now
  shutdown -H now
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_start_with_no_wait() {
        let parsed = parse(["svctl", "--no-wait", "start", "api"]).expect("parse");
        let ParseOutcome::Run(invocation) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            invocation.command,
            Command::Service {
                action: ServiceAction::Start,
                service: "api".to_string(),
                wait: false,
            },
        );
    }

    #[test]
    fn parses_operation_status_alias() {
        let parsed = parse(["svctl", "op", "018f"]).expect("parse");
        let ParseOutcome::Run(invocation) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            invocation.command,
            Command::OperationStatus {
                operation_id: "018f".to_string(),
            },
        );
    }

    #[test]
    fn parses_classic_shutdown_immediate_reboot() {
        let parsed = parse(["shutdown", "-r", "now"]).expect("parse");
        let ParseOutcome::Run(invocation) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            invocation.command,
            Command::Shutdown {
                kind: ShutdownKind::Reboot,
            },
        );
    }

    #[test]
    fn rejects_wait_on_status() {
        let error = parse(["svctl", "--wait", "status", "api"]).expect_err("usage");
        assert!(error.message.contains("not valid for status"));
    }
}
