use std::path::Path;

use crate::boundary::BoundaryError;
use crate::control::socket::{CONTROL_SOCKET_PATH, LinuxControlSocket};
use crate::init::{Phase1Infrastructure, Phase1InfrastructureWarning};
use crate::jobs::socket::{JOBS_SOCKET_PATH, LinuxJobsSocket};

use super::loopback::{LinuxLoopbackNetlink, bring_up_loopback};

pub(super) fn setup_linux_phase1_infrastructure() -> Result<Phase1Infrastructure, BoundaryError> {
    let control_socket = bind_control_socket(Path::new(CONTROL_SOCKET_PATH))?;
    let jobs_socket = bind_jobs_socket(Path::new(JOBS_SOCKET_PATH))?;
    let mut infrastructure = Phase1Infrastructure::new();
    infrastructure.set_control_socket(control_socket);
    infrastructure.set_jobs_socket(jobs_socket);
    let mut loopback = LinuxLoopbackNetlink;
    if let Err(error) = bring_up_loopback(&mut loopback) {
        infrastructure.push_warning(Phase1InfrastructureWarning::LoopbackBringUp {
            interface: "lo".to_string(),
            message: format!("{error:?}"),
        });
    }
    Ok(infrastructure)
}

/// The control socket is reachable by SYSTEM and Administrators.
///
/// peinit's own default ControlSecurity grants Administrators full access, and
/// its default ServiceSecurity grants them query and stop -- but both ACEs
/// were unreachable while the socket carried the Phase 1 `/run` seed, which is
/// SYSTEM only. `connect()` is checked by `inode_permission` before
/// `kacs_open_peer_token` and AccessCheck ever run, so an admin token was
/// refused before its descriptor was consulted, making the whole
/// ServiceSecurity layer decorative for any non-SYSTEM principal.
///
/// This is the reachability gate only. What an admin may then *do* is still
/// decided by ControlSecurity and ServiceSecurity, as it always was.
const CONTROL_SOCKET_SDDL: &str = "O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)";

/// The jobs socket admits every authenticated principal (PSPU §7.A).
///
/// Being able to connect *is* the permission to submit: the kernel checks
/// this descriptor at `connect()` and peinit performs no submit-time check of
/// its own. `FW` is the file-write generic right, which is what a Unix
/// `connect()` on a pathname socket needs. What a submitter may then do to a
/// job is decided by that job's own descriptor, not by this one.
const JOBS_SOCKET_SDDL: &str = "O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FW;;;AU)";

fn bind_jobs_socket(path: &Path) -> Result<LinuxJobsSocket, BoundaryError> {
    let socket = LinuxJobsSocket::bind(path).map_err(|error| {
        BoundaryError::Recovery(format!(
            "bind jobs socket {} failed: {error:?}",
            path.display()
        ))
    })?;
    crate::boundary::set_path_security(path, JOBS_SOCKET_SDDL).map_err(|error| {
        BoundaryError::Recovery(format!(
            "set jobs socket security on {} failed: {error}",
            path.display()
        ))
    })?;
    Ok(socket)
}

fn bind_control_socket(path: &Path) -> Result<LinuxControlSocket, BoundaryError> {
    if let Some(parent) = path.parent() {
        crate::boundary::ensure_runtime_directory(parent, CONTROL_SOCKET_SDDL).map_err(
            |error| {
                BoundaryError::Recovery(format!(
                    "create control socket dir {} failed: {error}",
                    parent.display()
                ))
            },
        )?;
    }
    let socket = LinuxControlSocket::bind(path).map_err(|error| {
        BoundaryError::Recovery(format!(
            "bind control socket {} failed: {error:?}",
            path.display()
        ))
    })?;
    // After bind, and before anything can connect: the socket inode inherits
    // the directory's descriptor at creation, but state it explicitly so the
    // socket does not depend on the parent staying as provisioned.
    crate::boundary::set_path_security(path, CONTROL_SOCKET_SDDL).map_err(|error| {
        BoundaryError::Recovery(format!(
            "set control socket security on {} failed: {error}",
            path.display()
        ))
    })?;
    Ok(socket)
}
