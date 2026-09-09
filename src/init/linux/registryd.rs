use std::io;
use std::path::Path;

use crate::boundary::{
    BoundaryError, Clock, LinuxMonotonicClock, LinuxProcessController, LinuxProcessLauncher,
    LinuxSystemTokenProvider, ProcessLauncher, ProcessSetupStatus, RegistryClient,
};
use crate::console_style::{ConsoleTag, relay_lines, render};
use crate::notify::{NotifySocket, NotifySocketReadError};
use crate::supervisor::{
    Supervisor, SupervisorError, SupervisorLaunchDispatch, SupervisorProcessSetupDispatch,
    SupervisorServiceLaunchDispatch,
};

use super::recovery_console::write_console;

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
        // Stamped as early as bind allows -- by path, because the fd form
        // answers EOPNOTSUPP for a socket; see `bind_secured`. Every service
        // writes its readiness notification here, so the grantee is the
        // Service group rather than a list of principals: membership follows
        // from having been started as a service, which is exactly the
        // population with something to say on this socket, and one an ordinary
        // user process cannot join.
        |path| crate::boundary::set_path_security(path, crate::notify::NOTIFY_SOCKET_SDDL),
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
    // Every failure from here on relays what registryd printed before returning.
    // These are exactly the cases an operator cannot diagnose from peinit's own
    // message: the daemon started, so the fault is inside it.
    if let Err(error) = wait_for_registryd_ready(
        supervisor,
        &notify_socket,
        &mut wait_clock,
        observed_at_ns.saturating_add(REGISTRYD_READY_TIMEOUT_NS),
    ) {
        report_registryd_output(&launch.launch.process);
        return Err(error);
    }
    // With registryd serving, ensure the base service-registry structure exists
    // before anything reads it. On a fresh system this creates
    // Machine\System\Services + SchemaVersion; on later boots it is a no-op.
    //
    // A failure here means registryd said READY=1 and then could not serve,
    // which is the case where its own words matter most.
    if let Err(error) = registry
        .provision_base_registry()
        .and_then(|_| registry.read_services_schema_version())
    {
        report_registryd_output(&launch.launch.process);
        return Err(error);
    }
    supervisor.retain_service_launch_for_runtime(launch.launch.clone());
    Ok(())
}

/// Relay whatever registryd printed before it failed, to the console, tagged.
///
/// The gap this closes: peinit gives every service capture pipes and drains
/// them in the runtime loop, but Phase 1 has no loop. A registryd that printed
/// exactly why it could not serve was therefore reported to the operator as
/// nothing but `registryd readiness timeout expired before READY=1`, and the
/// reason went into a pipe nobody ever read. loregd worked around this by
/// opening /dev/console and pointing its logger there — which fixed the
/// silence, but also took its output out of the pipe, so eventd never received
/// its startup lines either.
///
/// Failure path only. On success the fds are retained for the runtime, which
/// drains them into the pre-eventd buffer and on to eventd, exactly as it does
/// for every other service. peinit still does not echo service output to the
/// console during a normal boot.
///
/// Safe to call with PID 1's stack: the read ends are non-blocking
/// (`create_output_pipe`), so a registryd that is alive and silent yields
/// `EAGAIN` rather than hanging the boot.
fn report_registryd_output(process: &crate::boundary::LaunchedProcess) {
    let mut said = false;
    for (fd, stream) in [(process.stdout_fd, "stdout"), (process.stderr_fd, "stderr")] {
        let Some(fd) = fd else { continue };
        let captured = drain_nonblocking(fd);
        for (tag, line) in relay_lines(&format!("registryd({stream})"), &captured) {
            if !said {
                let _ = write_console(&render(
                    ConsoleTag::Failed,
                    "peinit: registryd failed; what it said follows\n",
                ));
                said = true;
            }
            let _ = write_console(&render(tag, &line));
        }
    }
    if !said {
        let _ = write_console(&render(
            ConsoleTag::Failed,
            "peinit: registryd failed and printed nothing\n",
        ));
    }
}

/// Read everything currently buffered on a non-blocking fd.
///
/// Stops at `EAGAIN` (nothing more right now) rather than at EOF, because the
/// child may still be alive — this runs on a failure path where waiting for it
/// to exit is the last thing the operator wants. Bounded, so a registryd that
/// died mid-flood cannot fill PID 1's memory with its last words.
fn drain_nonblocking(fd: i32) -> String {
    const CHUNK: usize = 4096;
    let cap = crate::console_style::MAX_RELAYED_LINE * crate::console_style::MAX_RELAYED_LINES;
    let mut out = Vec::new();
    let mut buffer = [0u8; CHUNK];
    loop {
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr() as *mut libc::c_void, CHUNK) };
        if read <= 0 {
            break;
        }
        out.extend_from_slice(&buffer[..read as usize]);
        if out.len() >= cap {
            out.truncate(cap);
            break;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
    //
    // **Each level explicitly.** `ensure_runtime_directory` creates one
    // directory and requires its parent to already exist -- peinit does not
    // create ancestors, deliberately, because a directory nobody named is a
    // directory whose descriptor nobody chose. `create_dir_all` used to hide
    // that here: at this point in Phase 1 only `/run` exists, and swapping in
    // the descriptor-aware call without walking the chain took PID 1 into
    // recovery on `/run/services`.
    for level in ancestors_under_run(parent) {
        crate::boundary::ensure_runtime_directory(
            &level,
            super::infrastructure::SERVICES_RUNTIME_DIR_SDDL,
        )
        .map_err(|error| {
            BoundaryError::Recovery(format!(
                "create notify socket directory {} failed: {error}",
                level.display()
            ))
        })?;
    }
    Ok(())
}

/// Every directory from `/run` down to `path`, `/run` itself excluded.
///
/// `/run` is seeded in Phase 1 before this runs (§2.1) and is not ours to
/// re-describe; everything below it is.
fn ancestors_under_run(path: &Path) -> Vec<std::path::PathBuf> {
    let mut levels: Vec<std::path::PathBuf> = path
        .ancestors()
        .take_while(|p| {
            p.as_os_str() != "/run" && p.as_os_str() != "/" && !p.as_os_str().is_empty()
        })
        .map(std::path::Path::to_path_buf)
        .collect();
    levels.reverse();
    levels
}

#[cfg(test)]
mod parent_tests {
    use super::ancestors_under_run;
    use std::path::Path;

    /// The bug this exists for: `/run/services` did not exist at Phase 1, and
    /// creating only the leaf took PID 1 into recovery.
    #[test]
    fn every_level_below_run_is_created_outermost_first() {
        let levels = ancestors_under_run(Path::new("/run/services/peinit"));
        let names: Vec<&str> = levels.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(names, vec!["/run/services", "/run/services/peinit"]);
    }

    /// `/run` is Phase 1's, seeded before this runs, and not ours to restamp.
    #[test]
    fn run_itself_is_left_alone() {
        assert!(ancestors_under_run(Path::new("/run")).is_empty());
    }
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
