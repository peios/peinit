use std::collections::BTreeMap;

use crate::boundary::{EnvironmentVariable, ProcessInheritedFd};
use crate::job::JobRecord;
use crate::security::SYSTEM_IDENTITY;
use crate::service::{ServiceDefinition, ServiceEnvironmentVariable};

pub const PATH: &str = "PATH";
pub const NOTIFY_SOCKET: &str = "NOTIFY_SOCKET";
pub const LISTEN_FDS: &str = "LISTEN_FDS";
pub const LISTEN_FDNAMES: &str = "LISTEN_FDNAMES";
pub const DEFAULT_PATH: &str = "/sbin:/bin";

pub fn build_launch_environment(
    job: &JobRecord,
    notify_socket_path: &str,
) -> Vec<EnvironmentVariable> {
    build_launch_environment_with_inherited_fds(job, notify_socket_path, &[], &[])
}

pub fn build_launch_environment_with_inherited_fds(
    job: &JobRecord,
    notify_socket_path: &str,
    global_environment: &[ServiceEnvironmentVariable],
    inherited_fds: &[ProcessInheritedFd],
) -> Vec<EnvironmentVariable> {
    let mut variables = BTreeMap::new();
    variables.insert(PATH.to_string(), DEFAULT_PATH.to_string());
    if !uses_compiled_in_environment_only(job) {
        for variable in global_environment {
            variables.insert(variable.name.clone(), variable.value.clone());
        }
    }
    for variable in &job.environment {
        variables.insert(variable.name.clone(), variable.value.clone());
    }
    variables.insert(NOTIFY_SOCKET.to_string(), notify_socket_path.to_string());
    if !inherited_fds.is_empty() {
        variables.insert(LISTEN_FDS.to_string(), inherited_fds.len().to_string());
        variables.insert(
            LISTEN_FDNAMES.to_string(),
            inherited_fds
                .iter()
                .map(|fd| fd.name.as_str())
                .collect::<Vec<_>>()
                .join(":"),
        );
    }
    variables
        .into_iter()
        .map(|(name, value)| EnvironmentVariable { name, value })
        .collect()
}

fn uses_compiled_in_environment_only(job: &JobRecord) -> bool {
    job.resolved_identity == SYSTEM_IDENTITY
        && job
            .service
            .as_deref()
            .is_some_and(is_early_platform_service)
}

fn is_early_platform_service(service: &str) -> bool {
    matches!(
        service,
        ServiceDefinition::REGISTRYD_NAME | "eudev" | "lpsd" | "authd" | "eventd"
    )
}

#[cfg(test)]
mod tests;
