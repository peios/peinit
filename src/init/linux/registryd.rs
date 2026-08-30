use std::io;
use std::path::Path;

use crate::boundary::{
    BoundaryError, Clock, LinuxMonotonicClock, LinuxProcessController, LinuxProcessLauncher,
    LinuxSystemTokenProvider, ProcessLauncher, ProcessSetupStatus, RegistryClient,
};
use crate::notify::{NotifySocket, NotifySocketReadError};
use crate::supervisor::{
    Supervisor, SupervisorError, SupervisorLaunchDispatch, SupervisorProcessSetupDispatch,
    SupervisorServiceLaunchDispatch,
};

const REGISTRYD_READY_TIMEOUT_NS: u64 = 30_000_000_000;
const REGISTRYD_SETUP_TIMEOUT_NS: u64 = 30_000_000_000;

pub(super) fn start_linux_phase1_registryd(
    supervisor: &mut Supervisor,
    registry: &mut dyn RegistryClient,
    observed_at_ns: u64,
) -> Result<(), BoundaryError> {
    ensure_notify_socket_parent(supervisor.settings().notify_socket_path.as_str())?;
    let notify_socket = NotifySocket::bind_secured(
        &supervisor.settings().notify_socket_path,
        // Stamped between bind and first use, so the socket is never reachable
        // under the descriptor it inherited. Every service writes its readiness
        // notification here, so the grantee is the Service group rather than a
        // list of principals: membership follows from having been started as a
        // service, which is exactly the population that has something to say on
        // this socket, and which an ordinary user process cannot join.
        |fd| crate::boundary::set_fd_security(fd, NOTIFY_SOCKET_SDDL),
    )
    .map_err(|error| {
        BoundaryError::Recovery(format!("bind registryd notify socket failed: {error:?}"))
    })?;
    let mut token_provider = LinuxSystemTokenProvider::new();
    let mut process_launcher = LinuxProcessLauncher::new();
    let mut launch_clock = FixedClock(observed_at_ns);

    supervisor
        .prepare_phase1_registryd_start(observed_at_ns)
        .map_err(supervisor_error)?;

    let mut controller = LinuxProcessController::new();
    let launch = match supervisor
        .launch_next_pending_service_job(
            &mut token_provider,
            &mut process_launcher,
            &mut launch_clock,
        )
        .map_err(supervisor_error)?
    {
        // Synchronous launchers (the in-crate test doubles) report the process
        // as started in one step.
        Some(SupervisorServiceLaunchDispatch::Launched(launch)) => *launch,
        // The real Linux launcher fork+execs and reports the result later over
        // the child's exec-status pipe — a two-phase launch. In the full
        // runtime the epoll loop drives this via the registered setup_status_fd;
        // Phase 1 has no loop running yet, so we drive the same completion
        // synchronously here (poll the fd, read the status, apply it) before
        // waiting on READY=1.
        Some(SupervisorServiceLaunchDispatch::PendingSetup(pending)) => {
            complete_phase1_pending_setup(
                supervisor,
                &mut process_launcher,
                &mut controller,
                pending.setup_status_fd,
                observed_at_ns,
            )?
        }
        Some(SupervisorServiceLaunchDispatch::Failed(failure)) => {
            return Err(BoundaryError::Recovery(format!(
                "registryd launch failed before exec: {:?}",
                failure.failure
            )));
        }
        None => {
            return Err(BoundaryError::Recovery(
                "registryd launch was not queued (boot plan produced no pending start)".to_string(),
            ));
        }
    };
    if launch.launch.job_event.service.as_deref()
        != Some(crate::service::ServiceDefinition::REGISTRYD_NAME)
    {
        return Err(BoundaryError::Recovery(format!(
            "Phase 1 launch queued unexpected service {:?}",
            launch.launch.job_event.service
        )));
    }

    let mut wait_clock = LinuxMonotonicClock::new();
    wait_for_registryd_ready(
        supervisor,
        &notify_socket,
        &mut wait_clock,
        observed_at_ns.saturating_add(REGISTRYD_READY_TIMEOUT_NS),
    )?;
    // With registryd serving, ensure the base service-registry structure exists
    // before anything reads it. On a fresh system this creates
    // Machine\System\Services + SchemaVersion; on later boots it is a no-op.
    registry.provision_base_registry()?;
    registry.read_services_schema_version()?;
    supervisor.retain_service_launch_for_runtime(launch.launch.clone());
    Ok(())
}

fn wait_for_registryd_ready<C>(
    supervisor: &mut Supervisor,
    notify_socket: &NotifySocket,
    clock: &mut C,
    deadline_ns: u64,
) -> Result<(), BoundaryError>
where
    C: Clock + ?Sized,
{
    loop {
        match notify_socket.receive() {
            Ok(datagram) => {
                let observed_at_ns = clock.monotonic_ns()?;
                let mut controller = LinuxProcessController::new();
                supervisor
                    .apply_notify_datagram(datagram, observed_at_ns, &mut controller)
                    .map_err(supervisor_error)?;
                if registryd_is_active(supervisor) {
                    return Ok(());
                }
            }
            Err(NotifySocketReadError::WouldBlock) => {
                let now_ns = clock.monotonic_ns()?;
                if now_ns >= deadline_ns {
                    let mut controller = LinuxProcessController::new();
                    let _ = supervisor.process_next_due_readiness_timeout(&mut controller, now_ns);
                    return Err(BoundaryError::Recovery(
                        "registryd readiness timeout expired before READY=1".to_string(),
                    ));
                }
                poll_fd_readable(
                    notify_socket.as_raw_fd(),
                    deadline_ns.saturating_sub(now_ns),
                )?;
            }
            Err(error) => {
                return Err(BoundaryError::Recovery(format!(
                    "receive registryd notify datagram failed: {error:?}"
                )));
            }
        }
    }
}

fn registryd_is_active(supervisor: &Supervisor) -> bool {
    supervisor
        .service_status(crate::service::ServiceDefinition::REGISTRYD_NAME)
        .is_ok_and(|status| status.state == crate::service::runtime::ServiceState::Active)
}

/// Drive the real Linux launcher's two-phase launch to completion for the
/// Phase-1 registryd: poll the child's exec-status fd until it reports a
/// terminal result, read that result, and apply it to the supervisor. Returns
/// the started-launch dispatch on success, mirroring what the runtime's epoll
/// loop does in `process_process_setup_event` — but synchronously, because the
/// runtime loop is not yet running this early in boot.
fn complete_phase1_pending_setup(
    supervisor: &mut Supervisor,
    launcher: &mut LinuxProcessLauncher,
    controller: &mut LinuxProcessController,
    setup_status_fd: i32,
    observed_at_ns: u64,
) -> Result<SupervisorLaunchDispatch, BoundaryError> {
    let mut clock = LinuxMonotonicClock::new();
    let deadline_ns = observed_at_ns.saturating_add(REGISTRYD_SETUP_TIMEOUT_NS);
    loop {
        let status = launcher
            .read_process_setup_status(setup_status_fd)
            .map_err(|error| {
                BoundaryError::Recovery(format!("read registryd setup status failed: {error:?}"))
            })?;
        if matches!(status, ProcessSetupStatus::Pending) {
            let now_ns = clock.monotonic_ns()?;
            if now_ns >= deadline_ns {
                return Err(BoundaryError::Recovery(
                    "registryd process setup did not complete before timeout".to_string(),
                ));
            }
            poll_fd_readable(setup_status_fd, deadline_ns.saturating_sub(now_ns))?;
            continue;
        }
        let now_ns = clock.monotonic_ns()?;
        let dispatch = supervisor
            .process_pending_process_setup_status(setup_status_fd, status, now_ns, controller)
            .map_err(supervisor_error)?;
        close_fd(setup_status_fd);
        return match dispatch {
            SupervisorProcessSetupDispatch::ServiceMainLaunched(launch) => Ok(*launch),
            SupervisorProcessSetupDispatch::ServiceMainFailed(failure) => Err(
                BoundaryError::Recovery(format!("registryd exec failed: {:?}", failure.failure)),
            ),
            other => Err(BoundaryError::Recovery(format!(
                "registryd process setup produced unexpected dispatch: {other:?}"
            ))),
        };
    }
}

fn close_fd(fd: i32) {
    if fd >= 0 {
        // SAFETY: closing an owned setup-status fd exactly once on the terminal
        // path; the supervisor has already cleared it from its pending map.
        unsafe {
            libc::close(fd);
        }
    }
}

fn poll_fd_readable(fd: i32, remaining_ns: u64) -> Result<(), BoundaryError> {
    let timeout_ms = remaining_ns
        .saturating_add(999_999)
        .saturating_div(1_000_000)
        .min(i32::MAX as u64) as i32;
    let mut fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let rc = unsafe { libc::poll(&mut fd, 1, timeout_ms) };
        if rc >= 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(BoundaryError::Recovery(format!(
            "poll registryd notify socket failed: {error}"
        )));
    }
}

/// Who may write a readiness notification.
///
/// `FW` rather than `GW`, following the jobs socket: it is the file-write
/// generic right, which is what a write to a pathname socket needs.
///
/// Administrators are deliberately absent, unlike the control socket. An
/// administrator has no business asserting that a service is ready, and the two
/// sockets have different populations however alike their paths look.
const NOTIFY_SOCKET_SDDL: &str = "O:SYG:SYD:(A;;GA;;;SY)(A;;FW;;;SU)";

fn ensure_notify_socket_parent(path: &str) -> Result<(), BoundaryError> {
    let Some(parent) = Path::new(path).parent() else {
        return Ok(());
    };
    // Through `ensure_runtime_directory` rather than `create_dir_all`, and with
    // the descriptor shared with the control socket that lands in the same
    // directory later in boot -- see SERVICES_RUNTIME_DIR_SDDL. A bare
    // create_dir_all leaves the directory inheriting the Phase 1 /run seed,
    // which is SYSTEM-only, so no service could traverse to the socket however
    // the socket itself was stamped.
    crate::boundary::ensure_runtime_directory(parent, super::infrastructure::SERVICES_RUNTIME_DIR_SDDL)
        .map_err(|error| {
            BoundaryError::Recovery(format!(
                "create notify socket directory {} failed: {error}",
                parent.display()
            ))
        })
}

fn supervisor_error(error: SupervisorError) -> BoundaryError {
    BoundaryError::Recovery(format!("registryd supervisor setup failed: {error:?}"))
}

#[derive(Debug, Clone, Copy)]
struct FixedClock(u64);

impl Clock for FixedClock {
    fn monotonic_ns(&mut self) -> Result<u64, BoundaryError> {
        Ok(self.0)
    }
}
