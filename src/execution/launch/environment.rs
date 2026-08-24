use std::collections::BTreeMap;

use crate::boundary::{EnvironmentVariable, ProcessInheritedFd};
use crate::job::JobRecord;
use crate::security::SYSTEM_IDENTITY;
use crate::service::{ServiceDefinition, ServiceEnvironmentVariable};

pub const PATH: &str = "PATH";
pub const NOTIFY_SOCKET: &str = "NOTIFY_SOCKET";
pub const LISTEN_FDS: &str = "LISTEN_FDS";
pub const LISTEN_FDNAMES: &str = "LISTEN_FDNAMES";
pub const LISTEN_PID: &str = "LISTEN_PID";
pub const DEFAULT_PATH: &str = "/sbin:/bin";

/// The protocol variables, which PSPU §4.20 reserves to peinit.
///
/// All four MUST be absent when nothing is passed, and no configurable layer
/// may set any of them.
///
/// Insertion order alone does not enforce that. It is enough for
/// `NOTIFY_SOCKET`, which is inserted unconditionally after both configurable
/// layers — but the `LISTEN_*` inserts are guarded on there being descriptors
/// to inject, and with `FdStoreMax` defaulting to 0 that is every service by
/// default. So a `Machine\System\Init\EnvVars\LISTEN_FDS=3` used to reach
/// every fd-store-less service untouched, pointing its `sd_listen_fds`-style
/// code at whatever happened to sit at descriptor 3.
///
/// `LISTEN_PID` is here too even though the child appends it after the clone
/// rather than going through this map: a configurable layer setting it would
/// put a second `LISTEN_PID=` in the block ahead of the real one, and `getenv`
/// returns the first match.
const PROTOCOL_VARIABLES: [&str; 4] = [NOTIFY_SOCKET, LISTEN_FDS, LISTEN_FDNAMES, LISTEN_PID];

fn is_protocol_variable(name: &str) -> bool {
    PROTOCOL_VARIABLES.contains(&name)
}

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
            if is_protocol_variable(&variable.name) {
                continue;
            }
            variables.insert(variable.name.clone(), variable.value.clone());
        }
    }
    for variable in &job.environment {
        if is_protocol_variable(&variable.name) {
            continue;
        }
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

/// registryd alone launches without the global `EnvVars` layer.
///
/// Not an availability rule — by Phase 2 the key has been read and is sitting
/// in supervisor state, so every other service can have it. This is a trust
/// rule, and it is circular precedence that motivates it: registryd *serves*
/// `Machine\\System\\Init\\EnvVars`, and write access to that key is
/// equivalent to compromising every service peinit starts (`LD_PRELOAD` and
/// friends go in unfiltered — the key's security descriptor is the whole
/// control boundary). Letting the key inject into the daemon that serves it
/// would make that boundary self-referential: whoever could write it could
/// subvert the process enforcing who may write it.
///
/// At the Phase 1 launch this is a no-op — `global_environment` is still empty
/// until `run_phase2_boot` fills it. The case it exists for is a registryd
/// *restart* after Phase 2, when the layer is populated and would otherwise
/// apply.
///
/// The SYSTEM check is part of the rule, not a shortcut: a non-SYSTEM job that
/// happened to be named `registryd` is not the platform's registry daemon and
/// gets the ordinary layering.
fn uses_compiled_in_environment_only(job: &JobRecord) -> bool {
    job.resolved_identity == SYSTEM_IDENTITY
        && job.service.as_deref() == Some(ServiceDefinition::REGISTRYD_NAME)
}

#[cfg(test)]
mod tests;
