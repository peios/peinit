//! The `submit` definition (PSPU §7.6): the fields, their defaults, and the
//! validation that answers `INVALID_ARGUMENTS` before anything is created.

use serde_json::{Map, Value};

use crate::service::{ServiceDefinition, ServiceEnvironmentVariable};

use super::model::JobReadiness;

/// The Linux `execve` bound on `argv` plus `envp` together (`ARG_MAX` on
/// every architecture Peios targets). Validated here so a submitter is told
/// `INVALID_ARGUMENTS` rather than getting a job that fails to exec.
const MAX_ARG_ENV_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedJobDefinition {
    pub image_path: String,
    pub arguments: Vec<String>,
    pub environment: Vec<ServiceEnvironmentVariable>,
    pub working_directory: String,
    pub description: String,
    /// Seconds the job may run before it is stopped; `0` is no limit.
    pub timeout_secs: u64,
    pub stop_timeout_secs: u64,
    pub readiness: JobReadiness,
    pub readiness_timeout_secs: u64,
    pub success_exit_codes: Vec<i32>,
    /// `LISTEN_FDNAMES` for the descriptors to inject, in attachment order.
    pub descriptor_names: Vec<String>,
    /// Whether the last attached descriptor is an output sink.
    pub output: bool,
    /// A submitter-supplied descriptor, in SDDL, or the default of §7.8.
    pub security_descriptor_sddl: Option<String>,
}

impl SubmittedJobDefinition {
    pub const DEFAULT_STOP_TIMEOUT_SECS: u64 = ServiceDefinition::DEFAULT_STOP_TIMEOUT_SECS;
    pub const DEFAULT_READINESS_TIMEOUT_SECS: u64 = ServiceDefinition::DEFAULT_START_TIMEOUT_SECS;

    /// How many attached descriptors the definition expects: the named ones,
    /// plus the sink.
    pub fn expected_descriptor_count(&self) -> usize {
        self.descriptor_names.len() + usize::from(self.output)
    }

    pub fn is_success_exit_code(&self, code: i32) -> bool {
        code == 0 || self.success_exit_codes.contains(&code)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmittedJobDefinitionError {
    /// The field, and why. The wire answer is `INVALID_ARGUMENTS` for all of
    /// them; the detail is for the message and the audit record.
    InvalidField { field: &'static str, reason: String },
    /// `descriptors` (plus `output`) does not match the attached count.
    DescriptorCountMismatch { expected: usize, attached: usize },
    ArgumentsTooLarge { bytes: usize, limit: usize },
}

impl SubmittedJobDefinitionError {
    pub fn message(&self) -> String {
        match self {
            Self::InvalidField { field, reason } => format!("invalid {field}: {reason}"),
            Self::DescriptorCountMismatch { expected, attached } => format!(
                "descriptors names {expected} descriptors but {attached} were attached"
            ),
            Self::ArgumentsTooLarge { bytes, limit } => {
                format!("arguments and environment total {bytes} bytes, over the {limit} limit")
            }
        }
    }
}

/// Parse and validate a `submit` request object against the count of
/// descriptors the kernel actually delivered with it.
pub fn parse_submitted_job_definition(
    object: &Map<String, Value>,
    attached_descriptors: usize,
) -> Result<SubmittedJobDefinition, SubmittedJobDefinitionError> {
    let image_path = required_path(object, "image_path")?;
    let arguments = optional_string_array(object, "arguments")?;
    let environment = optional_environment(object)?;
    let working_directory = match object.get("working_directory") {
        None | Some(Value::Null) => ServiceDefinition::DEFAULT_WORKING_DIRECTORY.to_string(),
        Some(_) => required_path(object, "working_directory")?,
    };
    let description = optional_string(object, "description")?.unwrap_or_default();
    let timeout_secs = optional_unsigned(object, "timeout")?.unwrap_or(0);
    let stop_timeout_secs = optional_unsigned(object, "stop_timeout")?
        .unwrap_or(SubmittedJobDefinition::DEFAULT_STOP_TIMEOUT_SECS);
    if stop_timeout_secs == 0 {
        return Err(invalid("stop_timeout", "must be at least 1"));
    }
    let readiness = match optional_string(object, "readiness")?.as_deref() {
        None | Some("none") => JobReadiness::None,
        Some("notify") => JobReadiness::Notify,
        Some(other) => return Err(invalid("readiness", format!("unknown value {other:?}"))),
    };
    let readiness_timeout_secs = optional_unsigned(object, "readiness_timeout")?
        .unwrap_or(SubmittedJobDefinition::DEFAULT_READINESS_TIMEOUT_SECS);
    let success_exit_codes = optional_exit_codes(object)?;
    let descriptor_names = optional_string_array(object, "descriptors")?;
    for name in &descriptor_names {
        if name.is_empty() {
            return Err(invalid("descriptors", "a name is empty"));
        }
        if name.contains(':') {
            return Err(invalid(
                "descriptors",
                format!("name {name:?} contains ':', the LISTEN_FDNAMES separator"),
            ));
        }
        if name.contains('\0') {
            return Err(invalid("descriptors", "a name contains NUL"));
        }
    }
    let output = optional_bool(object, "output")?.unwrap_or(false);
    let security_descriptor_sddl = optional_string(object, "security_descriptor")?;
    if security_descriptor_sddl
        .as_deref()
        .is_some_and(str::is_empty)
    {
        return Err(invalid("security_descriptor", "is empty"));
    }

    let definition = SubmittedJobDefinition {
        image_path,
        arguments,
        environment,
        working_directory,
        description,
        timeout_secs,
        stop_timeout_secs,
        readiness,
        readiness_timeout_secs,
        success_exit_codes,
        descriptor_names,
        output,
        security_descriptor_sddl,
    };

    let expected = definition.expected_descriptor_count();
    if expected != attached_descriptors {
        return Err(SubmittedJobDefinitionError::DescriptorCountMismatch {
            expected,
            attached: attached_descriptors,
        });
    }
    let bytes = arg_env_bytes(&definition);
    if bytes > MAX_ARG_ENV_BYTES {
        return Err(SubmittedJobDefinitionError::ArgumentsTooLarge {
            bytes,
            limit: MAX_ARG_ENV_BYTES,
        });
    }
    Ok(definition)
}

fn invalid(field: &'static str, reason: impl Into<String>) -> SubmittedJobDefinitionError {
    SubmittedJobDefinitionError::InvalidField {
        field,
        reason: reason.into(),
    }
}

fn required_path(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<String, SubmittedJobDefinitionError> {
    let value = object
        .get(field)
        .ok_or_else(|| invalid(field, "is required"))?
        .as_str()
        .ok_or_else(|| invalid(field, "must be a string"))?;
    if value.is_empty() {
        return Err(invalid(field, "is empty"));
    }
    if !value.starts_with('/') {
        return Err(invalid(field, "must be absolute"));
    }
    if value.contains('\0') {
        return Err(invalid(field, "contains NUL"));
    }
    Ok(value.to_string())
}

fn optional_string(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<String>, SubmittedJobDefinitionError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            if value.contains('\0') {
                return Err(invalid(field, "contains NUL"));
            }
            Ok(Some(value.clone()))
        }
        Some(_) => Err(invalid(field, "must be a string")),
    }
}

fn optional_bool(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<bool>, SubmittedJobDefinitionError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(invalid(field, "must be a boolean")),
    }
}

fn optional_unsigned(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<u64>, SubmittedJobDefinitionError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| invalid(field, "must be a non-negative integer")),
    }
}

fn optional_string_array(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Vec<String>, SubmittedJobDefinitionError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                let value = value
                    .as_str()
                    .ok_or_else(|| invalid(field, "every element must be a string"))?;
                if value.contains('\0') {
                    return Err(invalid(field, "an element contains NUL"));
                }
                Ok(value.to_string())
            })
            .collect(),
        Some(_) => Err(invalid(field, "must be an array of strings")),
    }
}

fn optional_environment(
    object: &Map<String, Value>,
) -> Result<Vec<ServiceEnvironmentVariable>, SubmittedJobDefinitionError> {
    let field = "environment";
    match object.get(field) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Object(entries)) => entries
            .iter()
            .map(|(name, value)| {
                if name.is_empty() {
                    return Err(invalid(field, "a variable name is empty"));
                }
                if name.contains('=') {
                    return Err(invalid(field, format!("name {name:?} contains '='")));
                }
                if name.contains('\0') {
                    return Err(invalid(field, "a variable name contains NUL"));
                }
                let value = value
                    .as_str()
                    .ok_or_else(|| invalid(field, "every value must be a string"))?;
                if value.contains('\0') {
                    return Err(invalid(field, "a value contains NUL"));
                }
                Ok(ServiceEnvironmentVariable {
                    name: name.clone(),
                    value: value.to_string(),
                })
            })
            .collect(),
        Some(_) => Err(invalid(field, "must be an object of strings")),
    }
}

fn optional_exit_codes(
    object: &Map<String, Value>,
) -> Result<Vec<i32>, SubmittedJobDefinitionError> {
    let field = "success_exit_codes";
    match object.get(field) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_i64()
                    .filter(|code| (0..=255).contains(code))
                    .map(|code| code as i32)
                    .ok_or_else(|| invalid(field, "every element must be an integer in 0..=255"))
            })
            .collect(),
        Some(_) => Err(invalid(field, "must be an array of integers")),
    }
}

fn arg_env_bytes(definition: &SubmittedJobDefinition) -> usize {
    let argv = definition.image_path.len()
        + 1
        + definition
            .arguments
            .iter()
            .map(|argument| argument.len() + 1)
            .sum::<usize>();
    let envp = definition
        .environment
        .iter()
        .map(|variable| variable.name.len() + 1 + variable.value.len() + 1)
        .sum::<usize>();
    argv + envp
}
