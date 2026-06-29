use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::control::socket::CONTROL_SOCKET_PATH;
use crate::shutdown::ShutdownKind;

use super::command::{Command, Invocation, OutputMode, ServiceAction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    Run(Invocation),
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

        let arg = string_arg(&program, arg_os)?;
        if let Some(path) = arg.strip_prefix("--socket=") {
            if path.is_empty() {
                return Err(usage(&program, "--socket requires a path"));
            }
            global.socket_path = Some(PathBuf::from(path));
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
    let output = global.output.unwrap_or(OutputMode::Human);

    Ok(ParseOutcome::Run(Invocation {
        program,
        socket_path,
        output,
        command,
    }))
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
        "help" => Err(usage(program, "use --help to show usage")),
        other if other.starts_with('-') => Err(usage(program, format!("unknown option {other}"))),
        other => Err(usage(program, format!("unknown command {other}"))),
    }
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
