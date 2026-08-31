use crate::runtime::console::ConsoleMessage;
use crate::supervisor::{
    SupervisorPid1SignalFdTurn, SupervisorPowerButtonAction, SupervisorPowerButtonDispatch,
    SupervisorShutdownCgroupKillDispatch, SupervisorShutdownDispatch,
    SupervisorShutdownDriveDispatch, SupervisorShutdownFinalizationDispatch,
    SupervisorShutdownKillDispatch, SupervisorShutdownSignalAction,
    SupervisorShutdownSignalDispatch, SupervisorShutdownStopDispatch,
    SupervisorShutdownTerminalDispatch, SupervisorShutdownTimeoutDispatch,
};

use crate::runtime::console::{collect_shutdown_finalization_state_console_message, push_message};

pub(super) fn collect_pid1_signal_turn_console_messages(
    turn: &SupervisorPid1SignalFdTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    if let SupervisorPid1SignalFdTurn::Shutdown(dispatch) = turn {
        collect_shutdown_signal_dispatch_console_messages(dispatch, out);
    }
}

pub(super) fn collect_shutdown_dispatch_console_messages(
    dispatch: &SupervisorShutdownDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    push_message(
        out,
        format!("peinit: shutdown {:?} started\n", dispatch.runtime.kind),
    );
    for killed in &dispatch.killed_starting {
        collect_shutdown_kill_console_message(killed, out);
    }
    for stop in &dispatch.first_wave {
        collect_shutdown_stop_console_message(stop, out);
    }
    collect_shutdown_finalization_state_console_message(&dispatch.runtime.finalization, out);
}

pub(super) fn collect_power_button_dispatch_console_messages(
    dispatch: &SupervisorPowerButtonDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    match &dispatch.action {
        SupervisorPowerButtonAction::Graceful(shutdown) => {
            collect_shutdown_dispatch_console_messages(shutdown, out);
        }
        SupervisorPowerButtonAction::AlreadyInProgress { kind } => {
            push_message(
                out,
                format!("peinit: shutdown {kind:?} already in progress\n"),
            );
        }
    }
}

pub(super) fn collect_shutdown_drive_dispatch_console_messages(
    dispatch: &SupervisorShutdownDriveDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    if let Some(timeout) = &dispatch.timeout {
        collect_shutdown_timeout_console_messages(timeout, out);
    }
    if let Some(finalization) = &dispatch.finalization {
        collect_shutdown_finalization_dispatch_console_messages(finalization, out);
    }
}

pub(super) fn collect_shutdown_terminal_dispatch_console_messages(
    dispatch: &SupervisorShutdownTerminalDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    if let Some(service) = dispatch.job_event.service.as_deref() {
        push_message(out, format!("peinit: shutdown service {service} exited\n"));
    }
    for stop in &dispatch.next_wave {
        collect_shutdown_stop_console_message(stop, out);
    }
    collect_shutdown_finalization_state_console_message(&dispatch.finalization, out);
}

fn collect_shutdown_signal_dispatch_console_messages(
    dispatch: &SupervisorShutdownSignalDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    match &dispatch.action {
        SupervisorShutdownSignalAction::Graceful(shutdown) => {
            collect_shutdown_dispatch_console_messages(shutdown, out);
        }
        SupervisorShutdownSignalAction::Forced(immediate) => {
            push_message(out, "peinit: shutdown forced reboot requested\n");
            for killed in &immediate.killed_services {
                collect_shutdown_cgroup_kill_console_message(killed, out);
            }
            collect_shutdown_finalization_dispatch_console_messages(&immediate.finalization, out);
        }
        SupervisorShutdownSignalAction::AlreadyInProgress { kind } => {
            push_message(
                out,
                format!("peinit: shutdown {kind:?} already in progress\n"),
            );
        }
    }
}

fn collect_shutdown_timeout_console_messages(
    dispatch: &SupervisorShutdownTimeoutDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    if dispatch.global_timeout {
        push_message(out, "peinit: shutdown global timeout expired\n");
    }
    for killed in &dispatch.cgroup_kills {
        collect_shutdown_cgroup_kill_console_message(killed, out);
    }
    for abandoned in &dispatch.abandoned {
        push_message(
            out,
            format!("peinit: shutdown abandoned {}\n", abandoned.service),
        );
    }
    for stop in &dispatch.next_wave {
        collect_shutdown_stop_console_message(stop, out);
    }
    collect_shutdown_finalization_state_console_message(&dispatch.finalization, out);
}

fn collect_shutdown_finalization_dispatch_console_messages(
    dispatch: &SupervisorShutdownFinalizationDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    push_message(out, "peinit: shutdown finalizing\n");
    if let crate::shutdown::CleanupActionResult::Failed(message) = &dispatch.report.random_seed {
        push_message(
            out,
            format!("peinit warning: shutdown random seed save failed: {message}\n"),
        );
    }
    collect_shutdown_finalization_state_console_message(&dispatch.finalization, out);
}

fn collect_shutdown_kill_console_message(
    dispatch: &SupervisorShutdownKillDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    push_message(
        out,
        format!("peinit: shutdown killing {}\n", dispatch.service),
    );
}

fn collect_shutdown_cgroup_kill_console_message(
    dispatch: &SupervisorShutdownCgroupKillDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    push_message(
        out,
        format!("peinit: shutdown killing {}\n", dispatch.service),
    );
}

fn collect_shutdown_stop_console_message(
    dispatch: &SupervisorShutdownStopDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    if let Some(reason) = dispatch.unsubstantiated_deadline {
        // The operator needs to know this service was killed rather than
        // asked, and why — it is the difference between a clean stop and a
        // process losing whatever it was in the middle of.
        crate::runtime::console::push_error(
            out,
            format!(
                "peinit: shutdown cannot substantiate a stop timeout for {} ({}); \
                 killing it without a graceful period\n",
                dispatch.service, reason,
            ),
        );
        return;
    }
    if dispatch.already_stopping {
        push_message(
            out,
            format!("peinit: shutdown waiting for {}\n", dispatch.service),
        );
    } else {
        push_message(
            out,
            format!("peinit: shutdown stopping {}\n", dispatch.service),
        );
    }
}
