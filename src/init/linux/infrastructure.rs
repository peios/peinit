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

/// The descriptor on `/run/services/peinit`, which is the parent of *both*
/// peinit sockets.
///
/// One constant rather than one per socket, and that is load-bearing:
/// `ensure_directory` re-stamps a directory that already exists, so two call
/// sites installing different descriptors on the same path would silently
/// leave whichever ran last. Registryd binds the notify socket in Phase 1,
/// before this file's control socket exists, so the last writer would be the
/// control socket and services would quietly lose the traverse they need to
/// reach `notify.sock`.
///
/// Services get **traverse only** (`GX`). Reaching the directory is not
/// permission to write the socket in it; that is decided by the socket's own
/// descriptor.
///
/// `S-1-5-6` is written out rather than as its `SU` alias, deliberately. The
/// alias is newer than some libpeios this image may carry, and the alias table
/// lives in a separately versioned package that nothing here declares a
/// minimum version of -- so an image pairing this peinit with an older
/// libpeios parses the descriptor, fails, and takes PID 1 into recovery before
/// a console exists. That is exactly what happened the first time this was
/// written with `SU`. A literal SID has been understood by every version there
/// has ever been.
pub(super) const SERVICES_RUNTIME_DIR_SDDL: &str =
    "O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GX;;;S-1-5-6)";

/// peinit's runtime directory: the parent of the control and jobs sockets,
/// and of the notify socket unless `peios.notifysocket=` moved it. It is
/// created in Phase 1 whether or not the notify socket lives in it, because
/// the two sockets that always do are bound later in boot and need it
/// (PEI-804).
pub(super) const PEINIT_RUNTIME_DIR: &str = "/run/services/peinit";

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
        crate::boundary::ensure_runtime_directory(parent, SERVICES_RUNTIME_DIR_SDDL).map_err(
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
