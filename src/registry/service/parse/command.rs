use crate::execution::command::parse_executable_command;

use crate::registry::fields::Field;
use crate::registry::value::{
    RawRegistryValue, ServiceRegistryDecodeError, decode_multi_sz_field, decode_sz_field,
};

pub(in crate::registry::service) fn parse_executable_command_field(
    value: &RawRegistryValue,
    field: Field,
) -> Result<String, ServiceRegistryDecodeError> {
    let parsed = decode_sz_field(value, field.name())?;
    validate_executable_command(field, &parsed)?;
    Ok(parsed)
}

pub(in crate::registry::service) fn parse_executable_command_list(
    value: &RawRegistryValue,
    field: Field,
) -> Result<Vec<String>, ServiceRegistryDecodeError> {
    decode_multi_sz_field(value, field.name())?
        .into_iter()
        .map(|entry| {
            validate_executable_command(field, &entry)?;
            Ok(entry)
        })
        .collect()
}

pub(in crate::registry::service) fn validate_exec_reload(
    value: &str,
) -> Result<(), ServiceRegistryDecodeError> {
    if let Some(signal) = value.strip_prefix("signal:") {
        if is_accepted_reload_signal(signal) {
            return Ok(());
        }
        return Err(ServiceRegistryDecodeError::InvalidReloadSignal {
            value: value.to_string(),
        });
    }
    validate_executable_command(Field::ExecReload, value)
}

fn validate_executable_command(
    field: Field,
    value: &str,
) -> Result<(), ServiceRegistryDecodeError> {
    parse_executable_command(value)
        .map(|_| ())
        .map_err(
            |source| ServiceRegistryDecodeError::InvalidExecutableCommand {
                field: field.name(),
                value: value.to_string(),
                source,
            },
        )
}

fn is_accepted_reload_signal(signal: &str) -> bool {
    matches!(
        signal,
        "SIGHUP"
            | "SIGINT"
            | "SIGQUIT"
            | "SIGILL"
            | "SIGTRAP"
            | "SIGABRT"
            | "SIGBUS"
            | "SIGFPE"
            | "SIGUSR1"
            | "SIGSEGV"
            | "SIGUSR2"
            | "SIGPIPE"
            | "SIGALRM"
            | "SIGTERM"
            | "SIGSTKFLT"
            | "SIGCHLD"
            | "SIGCONT"
            | "SIGTSTP"
            | "SIGTTIN"
            | "SIGTTOU"
            | "SIGURG"
            | "SIGXCPU"
            | "SIGXFSZ"
            | "SIGVTALRM"
            | "SIGPROF"
            | "SIGWINCH"
            | "SIGIO"
            | "SIGPWR"
            | "SIGSYS"
    )
}
