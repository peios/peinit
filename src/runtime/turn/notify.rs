use crate::notify::NotifySocketReadError;
use crate::boundary::{Clock, ProcessController};
use crate::execution::notify::NotifyAppliedField;
use crate::shutdown::ShutdownError;
use crate::supervisor::{Supervisor, SupervisorError};

use super::deadline::sync_deadline_timer;
use super::model::{
    RuntimeNotifyDatagram, RuntimeNotifyRead, RuntimeNotifyRejection, RuntimeNotifySource,
    RuntimeNotifySupervisorTurn, RuntimeShutdownDeadlineTimer, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError,
};

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
    let datagram = match notify_source.read_notify_datagram() {
        Ok(Some(datagram)) => datagram,
        Ok(None) => {
            return Ok(RuntimeShutdownEventTurn::Notify {
                read: RuntimeNotifyRead::WouldBlock,
                supervisor: None,
                deadline_timer: None,
            });
        }
        // A truncated datagram is a rejection of that one message, not a
        // failure of the socket: it was consumed and is gone, and nothing from
        // it was applied. Recording it the way a malformed line is recorded is
        // what the all-or-nothing rule asks for; ending the runtime loop over
        // it would be a far worse answer than the silent truncation it
        // replaces.
        Err(NotifySocketReadError::Truncated { payload, control }) => {
            return Ok(RuntimeShutdownEventTurn::Notify {
                read: RuntimeNotifyRead::WouldBlock,
                supervisor: Some(RuntimeNotifySupervisorTurn::Rejected(
                    RuntimeNotifyRejection::Truncated { payload, control },
                )),
                deadline_timer: None,
            });
        }
        Err(error) => return Err(RuntimeShutdownEventTurnError::NotifyRead(error)),
    };

    let read_datagram = RuntimeNotifyDatagram::from_datagram(&datagram);
    let sender_pid = read_datagram.sender_pid;
    let read = RuntimeNotifyRead::Datagram(read_datagram);
    let observed_at_ns = clock.monotonic_ns().map_err(|error| {
        RuntimeShutdownEventTurnError::Supervisor(SupervisorError::Clock(error))
    })?;
    let supervisor_turn =
        match supervisor.apply_notify_datagram(datagram, observed_at_ns, controller) {
            Ok(dispatch) => RuntimeNotifySupervisorTurn::Applied(Box::new(dispatch)),
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
            Err(error) => return Err(RuntimeShutdownEventTurnError::Supervisor(error)),
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
