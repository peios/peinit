use crate::boundary::{Clock, LinuxPowerButtonRead, ProcessController, RealtimeClock};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::SystemAccessChecker;
use crate::runtime::{
    RuntimeEventRegistrar, RuntimePowerButtonSource, RuntimePowerButtonTurn,
    RuntimeShutdownDeadlineTimer, RuntimeShutdownEventContext, RuntimeShutdownEventTurn,
    RuntimeShutdownEventTurnError,
};
use crate::supervisor::{Supervisor, SupervisorPowerButtonAction};

pub(super) fn process_power_button_event<D, C, P, F, A, R>(
    supervisor: &mut Supervisor,
    fd: i32,
    power_button_source: &mut dyn RuntimePowerButtonSource,
    deadline_timer: &mut D,
    context: RuntimeShutdownEventContext<'_, C, P, F, A, R>,
) -> Result<RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError>
where
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: crate::boundary::ShutdownFinalizer,
    A: SystemAccessChecker
        + ServiceAccessChecker
        + crate::submitted::JobAccessChecker
        + crate::submitted::JobDescriptorFactory
        + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
{
    let turn = match power_button_source.read_power_button(fd) {
        Ok(LinuxPowerButtonRead::Pressed) => {
            let observed_at_ns = context
                .clock
                .monotonic_ns()
                .map_err(RuntimeShutdownEventTurnError::Clock)?;
            let dispatch = supervisor
                .handle_power_button(context.controller, observed_at_ns)
                .map_err(RuntimeShutdownEventTurnError::Supervisor)?;
            let deadline_timer =
                if matches!(dispatch.action, SupervisorPowerButtonAction::Graceful(_)) {
                    Some(
                        supervisor
                            .sync_shutdown_deadline_timer(deadline_timer)
                            .map_err(RuntimeShutdownEventTurnError::Supervisor)?,
                    )
                } else {
                    None
                };
            RuntimePowerButtonTurn::Shutdown {
                read: LinuxPowerButtonRead::Pressed,
                supervisor: Box::new(dispatch),
                deadline_timer,
            }
        }
        Ok(read) => RuntimePowerButtonTurn::Ignored { read },
        Err(error) => {
            let source_disabled = context.registrar.unregister_source(fd).is_ok();
            RuntimePowerButtonTurn::ReadFailed {
                fd,
                error,
                source_disabled,
            }
        }
    };

    Ok(RuntimeShutdownEventTurn::PowerButton { fd, turn })
}
