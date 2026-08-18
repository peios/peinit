mod control;
mod job;
mod shutdown;

use crate::runtime::console::ConsoleMessage;
use crate::runtime::{
    RuntimeCalendarTimerTurn, RuntimeNotifySupervisorTurn, RuntimePowerButtonTurn,
    RuntimeProcessSetupTurn, RuntimeShutdownEventTurn,
};

pub(super) fn collect_runtime_shutdown_turn_console_messages(
    turn: &RuntimeShutdownEventTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    match turn {
        RuntimeShutdownEventTurn::Pid1Signal {
            supervisor,
            child_reaps,
            drive,
            ..
        } => {
            shutdown::collect_pid1_signal_turn_console_messages(supervisor, out);
            for reap in child_reaps {
                job::collect_child_reap_turn_console_messages(reap, out);
            }
            if let Some(drive) = drive {
                shutdown::collect_shutdown_drive_dispatch_console_messages(drive, out);
            }
        }
        RuntimeShutdownEventTurn::ControlConnection { supervisor, .. } => {
            control::collect_control_connection_table_turn_console_messages(supervisor, out);
        }
        RuntimeShutdownEventTurn::ShutdownDeadlineTimer { drive, .. } => {
            if let Some(drive) = drive {
                shutdown::collect_shutdown_drive_dispatch_console_messages(drive, out);
            }
        }
        RuntimeShutdownEventTurn::LifecycleDeadlineTimer { drive, .. } => {
            if let Some(drive) = drive {
                job::collect_lifecycle_deadline_dispatch_console_messages(drive, out);
            }
        }
        RuntimeShutdownEventTurn::Notify {
            supervisor: Some(RuntimeNotifySupervisorTurn::Applied(dispatch)),
            ..
        } => control::collect_notify_dispatch_console_messages(dispatch, out),
        RuntimeShutdownEventTurn::CalendarTimer { turn, .. } => {
            control::collect_runtime_calendar_timer_console_messages(turn, out);
        }
        RuntimeShutdownEventTurn::FilesystemCheckHelper { turn, .. }
        | RuntimeShutdownEventTurn::FilesystemCheckHelperExit { turn, .. } => {
            control::collect_filesystem_check_helper_turn_console_messages(turn, out);
        }
        RuntimeShutdownEventTurn::ProcessSetup { turn, .. } => {
            collect_process_setup_turn_console_messages(turn, out);
        }
        RuntimeShutdownEventTurn::PowerButton { turn, .. } => {
            collect_power_button_turn_console_messages(turn, out);
        }
        RuntimeShutdownEventTurn::ControlListener { .. }
        | RuntimeShutdownEventTurn::IdleControlConnectionsClosed { .. }
        | RuntimeShutdownEventTurn::StaleControlConnection { .. }
        | RuntimeShutdownEventTurn::Notify { .. }
        | RuntimeShutdownEventTurn::ServiceLogPipe { .. }
        | RuntimeShutdownEventTurn::JfsDevice { .. }
        | RuntimeShutdownEventTurn::RegistryWatch { .. } => {}
    }
}

fn collect_power_button_turn_console_messages(
    turn: &RuntimePowerButtonTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    if let RuntimePowerButtonTurn::Shutdown { supervisor, .. } = turn {
        shutdown::collect_power_button_dispatch_console_messages(supervisor, out);
    }
}

fn collect_process_setup_turn_console_messages(
    turn: &RuntimeProcessSetupTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    let RuntimeProcessSetupTurn::Completed { supervisor, .. } = turn else {
        return;
    };
    match &**supervisor {
        crate::supervisor::SupervisorProcessSetupDispatch::ServiceMainLaunched(dispatch) => {
            crate::runtime::console::push_service_started(out, &dispatch.started.job_event);
            crate::runtime::console::collect_start_dispatches_console_messages(
                &dispatch.start_dispatches,
                out,
            );
        }
        crate::supervisor::SupervisorProcessSetupDispatch::ServiceMainFailed(dispatch) => {
            crate::runtime::console::push_service_launch_failed(out, &dispatch.job_event);
            crate::runtime::console::collect_start_failure_console_messages(&dispatch.failure, out);
            crate::runtime::console::collect_start_dispatches_console_messages(
                &dispatch.start_dispatches,
                out,
            );
        }
        crate::supervisor::SupervisorProcessSetupDispatch::StartHookFailed(dispatch) => {
            crate::runtime::console::collect_start_failure_console_messages(&dispatch.failure, out);
            crate::runtime::console::collect_start_dispatches_console_messages(
                &dispatch.start_dispatches,
                out,
            );
        }
        crate::supervisor::SupervisorProcessSetupDispatch::PostHookFailed(dispatch) => {
            job::collect_post_start_hook_terminal_console_messages(&dispatch.terminal, out);
            crate::runtime::console::collect_start_dispatches_console_messages(
                &dispatch.start_dispatches,
                out,
            );
        }
        crate::supervisor::SupervisorProcessSetupDispatch::HealthCheckFailed(dispatch) => {
            job::collect_health_check_terminal_console_messages(&dispatch.terminal, out);
        }
        _ => {}
    }
}

pub(super) fn collect_runtime_calendar_timer_console_messages(
    turn: &RuntimeCalendarTimerTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    control::collect_runtime_calendar_timer_console_messages(turn, out);
}

pub(super) fn collect_post_start_hook_terminal_console_messages(
    dispatch: &crate::execution::start::PostStartHookTerminalDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    job::collect_post_start_hook_terminal_console_messages(dispatch, out);
}

pub(super) fn collect_health_check_terminal_console_messages(
    dispatch: &crate::supervisor::SupervisorHealthCheckTerminalDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    job::collect_health_check_terminal_console_messages(dispatch, out);
}
