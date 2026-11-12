use crate::boundary::{Clock, ProcessController, ProcessLauncher};
use crate::execution::notify::NotifyAppliedField;
use crate::notify::{NotifyDatagram, NotifySocketReadError};
use crate::runtime::{RuntimeEventRegistrar, RuntimeServiceLogPipes};
use crate::shutdown::ShutdownError;
use crate::supervisor::{Supervisor, SupervisorError};

use super::deadline::sync_deadline_timer;
use super::model::{
    RuntimeNotifyDatagram, RuntimeNotifyRead, RuntimeNotifyRejection, RuntimeNotifySource,
    RuntimeNotifySupervisorTurn, RuntimeShutdownDeadlineTimer, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError,
};
use super::process_setup::process_process_setup_event;

/// What one notify-socket readiness produced, in the order it happened.
pub(crate) struct RuntimeNotifyEventTurns {
    /// The setup-status descriptor read ahead of the datagram, if any. Its
    /// own readiness may still be in the batch, and is not to be read again.
    pub(crate) setup_status_fd: Option<i32>,
    pub(crate) turns: Vec<RuntimeShutdownEventTurn>,
}

/// What the runtime needs to read a setup status ahead of its turn.
pub(crate) struct RuntimeNotifySetupReader<'a, L, R>
where
    L: ProcessLauncher + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
{
    pub(crate) launcher: &'a mut L,
    pub(crate) registrar: &'a mut R,
    pub(crate) log_pipes: &'a mut RuntimeServiceLogPipes,
}

/// Read one datagram and apply it, reading its sender's setup status first
/// when the sender is a launched process whose job does not carry its PID yet.
///
/// A job records its PID only when its setup status is processed, and the
/// status and the datagram are separate sources that a turn handles in
/// whatever order epoll returned them. A service fast enough to send
/// `READY=1` straight after exec could have it handled first, against a job
/// with no PID: refused as an unauthenticated sender, and never sent again,
/// so the service was killed at its start timeout (PEI-1688). The status is
/// always readable by then -- the setup pipe closes at exec, before the
/// program that sends the datagram runs -- so it is read here, before the
/// datagram is authenticated. Authentication itself is unchanged: the
/// datagram is applied only if the job now carries the sender's PID and the
/// PID is verified against the job's pidfd.
pub(crate) fn process_notify_event_setup_first<N, D, C, P, L, R>(
    supervisor: &mut Supervisor,
    notify_source: &mut N,
    deadline_timer: &mut D,
    clock: &mut C,
    controller: &mut P,
    setup_reader: Option<RuntimeNotifySetupReader<'_, L, R>>,
) -> Result<RuntimeNotifyEventTurns, RuntimeShutdownEventTurnError>
where
    N: RuntimeNotifySource + ?Sized,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
    L: ProcessLauncher + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
{
    let datagram = match read_notify_datagram(notify_source)? {
        Ok(datagram) => datagram,
        Err(turn) => {
            return Ok(RuntimeNotifyEventTurns {
                setup_status_fd: None,
                turns: vec![turn],
            });
        }
    };
    let sender_pid = datagram.credentials.pid;
    let mut turns = Vec::new();
    let mut setup_status_fd = None;
    if let (Some(fd), Some(reader)) = (
        supervisor.pending_process_setup_fd_for_pid(sender_pid),
        setup_reader,
    ) {
        turns.push(process_process_setup_event(
            supervisor,
            fd,
            reader.launcher,
            clock,
            controller,
            reader.registrar,
            reader.log_pipes,
        )?);
        setup_status_fd = Some(fd);
    }
    if supervisor
        .pending_process_setup_fd_for_pid(sender_pid)
        .is_none()
    {
        // Anything held for this sender came first, and goes first.
        for held in notify_source
            .held_notify_datagrams()
            .release(|pid| pid != sender_pid)
        {
            turns.push(apply_notify_datagram_turn(
                supervisor,
                held,
                deadline_timer,
                clock,
                controller,
            )?);
        }
    }
    turns.push(apply_or_hold_notify_datagram(
        supervisor,
        notify_source,
        datagram,
        deadline_timer,
        clock,
        controller,
    )?);
    Ok(RuntimeNotifyEventTurns {
        setup_status_fd,
        turns,
    })
}

/// Apply the held datagrams whose sender's setup has since been processed --
/// or has gone, in which case authentication now refuses them.
pub(crate) fn replay_held_notify_datagrams<N, D, C, P>(
    supervisor: &mut Supervisor,
    notify_source: &mut N,
    deadline_timer: &mut D,
    clock: &mut C,
    controller: &mut P,
) -> Result<Vec<RuntimeShutdownEventTurn>, RuntimeShutdownEventTurnError>
where
    N: RuntimeNotifySource + ?Sized,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
{
    let held = notify_source.held_notify_datagrams();
    if held.is_empty() {
        return Ok(Vec::new());
    }
    let released = held.release(|pid| supervisor.pending_process_setup_fd_for_pid(pid).is_some());
    let mut turns = Vec::with_capacity(released.len());
    for datagram in released {
        turns.push(apply_notify_datagram_turn(
            supervisor,
            datagram,
            deadline_timer,
            clock,
            controller,
        )?);
    }
    Ok(turns)
}

/// Read one datagram and apply it, as one event of its own.
///
/// This has no way to read a setup status, so a datagram from a launched
/// process whose job does not carry its PID yet is held instead.
pub(super) fn process_notify_event<N, D, C, P>(
    supervisor: &mut Supervisor,
    notify_source: &mut N,
    deadline_timer: &mut D,
    clock: &mut C,
    controller: &mut P,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    N: RuntimeNotifySource + ?Sized,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
{
    match read_notify_datagram(notify_source)? {
        Ok(datagram) => apply_or_hold_notify_datagram(
            supervisor,
            notify_source,
            datagram,
            deadline_timer,
            clock,
            controller,
        ),
        Err(turn) => Ok(turn),
    }
}

/// The datagram read, or the turn that reading it ended in.
fn read_notify_datagram<N>(
    notify_source: &mut N,
) -> Result<Result<NotifyDatagram, RuntimeShutdownEventTurn>, RuntimeShutdownEventTurnError>
where
    N: RuntimeNotifySource + ?Sized,
{
    match notify_source.read_notify_datagram() {
        Ok(Some(datagram)) => Ok(Ok(datagram)),
        Ok(None) => Ok(Err(RuntimeShutdownEventTurn::Notify {
            read: RuntimeNotifyRead::WouldBlock,
            supervisor: None,
            deadline_timer: None,
        })),
        // A truncated datagram is a rejection of that one message, not a
        // failure of the socket: it was consumed and is gone, and nothing from
        // it was applied. Recording it the way a malformed line is recorded is
        // what the all-or-nothing rule asks for; ending the runtime loop over
        // it would be a far worse answer than the silent truncation it
        // replaces.
        Err(NotifySocketReadError::Truncated { payload, control }) => {
            Ok(Err(RuntimeShutdownEventTurn::Notify {
                read: RuntimeNotifyRead::WouldBlock,
                supervisor: Some(RuntimeNotifySupervisorTurn::Rejected(
                    RuntimeNotifyRejection::Truncated { payload, control },
                )),
                deadline_timer: None,
            }))
        }
        Err(error) => Err(RuntimeShutdownEventTurnError::NotifyRead(error)),
    }
}

/// Hold a datagram whose sender's setup status has still not been processed;
/// apply any other.
fn apply_or_hold_notify_datagram<N, D, C, P>(
    supervisor: &mut Supervisor,
    notify_source: &mut N,
    datagram: NotifyDatagram,
    deadline_timer: &mut D,
    clock: &mut C,
    controller: &mut P,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    N: RuntimeNotifySource + ?Sized,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
{
    let sender_pid = datagram.credentials.pid;
    if supervisor
        .pending_process_setup_fd_for_pid(sender_pid)
        .is_none()
    {
        return apply_notify_datagram_turn(supervisor, datagram, deadline_timer, clock, controller);
    }
    let read = RuntimeNotifyRead::Datagram(RuntimeNotifyDatagram::from_datagram(&datagram));
    let supervisor_turn = match notify_source.held_notify_datagrams().hold(datagram) {
        Ok(()) => RuntimeNotifySupervisorTurn::HeldUntilSetup { sender_pid },
        Err(_) => RuntimeNotifySupervisorTurn::Rejected(RuntimeNotifyRejection::SetupPending),
    };
    Ok(RuntimeShutdownEventTurn::Notify {
        read,
        supervisor: Some(supervisor_turn),
        deadline_timer: None,
    })
}

fn apply_notify_datagram_turn<D, C, P>(
    supervisor: &mut Supervisor,
    datagram: NotifyDatagram,
    deadline_timer: &mut D,
    clock: &mut C,
    controller: &mut P,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
{
    let read_datagram = RuntimeNotifyDatagram::from_datagram(&datagram);
    let sender_pid = read_datagram.sender_pid;
    let read = RuntimeNotifyRead::Datagram(read_datagram);
    let observed_at_ns = clock.monotonic_ns().map_err(|error| {
        RuntimeShutdownEventTurnError::Supervisor(SupervisorError::Clock(error))
    })?;
    let supervisor_turn =
        match supervisor.apply_notify_datagram(datagram, observed_at_ns, controller) {
            Ok(crate::supervisor::SupervisorNotifyOutcome::Service(dispatch)) => {
                RuntimeNotifySupervisorTurn::Applied(dispatch)
            }
            Ok(crate::supervisor::SupervisorNotifyOutcome::SubmittedJob(dispatch)) => {
                RuntimeNotifySupervisorTurn::AppliedToJob(Box::new(dispatch))
            }
            Err(SupervisorError::NotifyParse(error)) => {
                let attribution = supervisor
                    .authenticate_notify_datagram_sender(sender_pid, controller)
                    .ok();
                RuntimeNotifySupervisorTurn::Rejected(RuntimeNotifyRejection::Parse {
                    error,
                    attribution,
                })
            }
            Err(SupervisorError::Notify(error)) => {
                RuntimeNotifySupervisorTurn::Rejected(RuntimeNotifyRejection::Apply {
                    error,
                    attribution: None,
                })
            }
            Err(SupervisorError::Shutdown(
                error @ ShutdownError::InvalidTimeoutExtension { .. },
            )) => RuntimeNotifySupervisorTurn::Rejected(RuntimeNotifyRejection::Shutdown(error)),
            // Anything else is peinit unable to act on one datagram. The
            // datagram is consumed and gone; what it was about is the
            // sender's service, and that is what fails (PEI-1125). A sender
            // that cannot be attributed fails nothing: the error is
            // announced and the datagram dropped.
            Err(error) => {
                let subject = supervisor
                    .authenticate_notify_datagram_sender(sender_pid, controller)
                    .ok()
                    .map(|sender| crate::supervisor::SupervisorInternalErrorSubject {
                        service: Some(sender.service),
                        job_id: Some(sender.job_id),
                    })
                    .unwrap_or_default();
                let dispatch = supervisor.fail_after_internal_error(
                    subject,
                    "notify",
                    format!("{error:?}"),
                    observed_at_ns,
                    controller,
                );
                RuntimeNotifySupervisorTurn::InternalError(Box::new(dispatch))
            }
        };
    let deadline_timer_turn = if notify_extended_shutdown_timeout(supervisor, &supervisor_turn) {
        Some(sync_deadline_timer(supervisor, deadline_timer)?)
    } else {
        None
    };

    Ok(RuntimeShutdownEventTurn::Notify {
        read,
        supervisor: Some(supervisor_turn),
        deadline_timer: deadline_timer_turn,
    })
}

fn notify_extended_shutdown_timeout(
    supervisor: &Supervisor,
    turn: &RuntimeNotifySupervisorTurn,
) -> bool {
    supervisor.shutdown().is_some()
        && matches!(
            turn,
            RuntimeNotifySupervisorTurn::Applied(dispatch)
                if dispatch.notify.applied_fields.iter().any(|field| {
                    matches!(field, NotifyAppliedField::ExtendTimeoutUsec { .. })
                })
        )
}
