use crate::supervisor::{
    SupervisorChildReapDispatch, SupervisorChildReapTurn, SupervisorHealthCheckTerminalDispatch,
    SupervisorLifecycleDeadlineDispatch, SupervisorPostStartHookTerminalDispatch,
    SupervisorPreStartHookTerminalDispatch, SupervisorReadinessTimeoutDispatch,
    SupervisorReloadCommandTimeoutDispatch, SupervisorTerminalDispatch,
    SupervisorWatchdogTimeoutDispatch,
};

use super::shutdown;
use crate::runtime::console::{
    collect_restart_start_dispatches_console_messages, collect_service_transition_console_message,
    collect_service_transitions_console_messages, collect_start_dispatches_console_messages,
    push_critical_service_failure,
};

pub(super) fn collect_child_reap_turn_console_messages(
    turn: &SupervisorChildReapTurn,
    out: &mut Vec<String>,
) {
    let SupervisorChildReapTurn::Tracked { dispatch, .. } = turn else {
        return;
    };
    match dispatch {
        SupervisorChildReapDispatch::Runtime(dispatch) => {
            collect_terminal_dispatch_console_messages(dispatch, out);
        }
        SupervisorChildReapDispatch::PreStartHook(dispatch) => {
            collect_pre_start_hook_terminal_console_messages(dispatch, out);
        }
        SupervisorChildReapDispatch::PostStartHook(dispatch) => {
            collect_post_start_hook_terminal_dispatch_console_messages(dispatch, out);
        }
        SupervisorChildReapDispatch::HealthCheck(dispatch) => {
            collect_health_check_terminal_console_messages(dispatch, out);
        }
        SupervisorChildReapDispatch::Shutdown(dispatch) => {
            shutdown::collect_shutdown_terminal_dispatch_console_messages(dispatch, out);
        }
        SupervisorChildReapDispatch::ReloadCommand(_) => {}
    }
}

pub(super) fn collect_lifecycle_deadline_dispatch_console_messages(
    dispatch: &SupervisorLifecycleDeadlineDispatch,
    out: &mut Vec<String>,
) {
    for timeout in &dispatch.pre_start_check_timeouts {
        collect_service_transitions_console_messages(
            &timeout.timeout.completion.service_transitions,
            out,
        );
        collect_start_dispatches_console_messages(&timeout.start_dispatches, out);
    }
    for timeout in &dispatch.pre_start_hook_timeouts {
        collect_service_transitions_console_messages(&timeout.timeout.service_transitions, out);
        collect_start_dispatches_console_messages(&timeout.start_dispatches, out);
    }
    for timeout in &dispatch.post_start_hook_timeouts {
        collect_service_transitions_console_messages(&timeout.timeout.service_transitions, out);
        collect_start_dispatches_console_messages(&timeout.start_dispatches, out);
    }
    for timeout in &dispatch.readiness_timeouts {
        collect_readiness_timeout_dispatch_console_messages(timeout, out);
    }
    for timeout in &dispatch.reload_command_timeouts {
        collect_reload_command_timeout_dispatch_console_messages(timeout, out);
    }
    for timeout in &dispatch.health_check_timeouts {
        collect_health_check_terminal_console_messages(&timeout.terminal, out);
    }
    for timeout in &dispatch.watchdog_timeouts {
        collect_watchdog_timeout_console_messages(timeout, out);
    }
}

pub(super) fn collect_post_start_hook_terminal_console_messages(
    dispatch: &crate::execution::start::PostStartHookTerminalDispatch,
    out: &mut Vec<String>,
) {
    collect_service_transitions_console_messages(&dispatch.service_transitions, out);
}

pub(super) fn collect_health_check_terminal_console_messages(
    dispatch: &SupervisorHealthCheckTerminalDispatch,
    out: &mut Vec<String>,
) {
    collect_service_transitions_console_messages(&dispatch.service_transitions, out);
    if dispatch.critical_reboot.is_some()
        && let Some(service) = dispatch.job_event.service.as_deref()
    {
        push_critical_service_failure(out, service, "health check failed");
    }
}

fn collect_readiness_timeout_dispatch_console_messages(
    dispatch: &SupervisorReadinessTimeoutDispatch,
    out: &mut Vec<String>,
) {
    collect_service_transitions_console_messages(&dispatch.timeout.service_transitions, out);
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

fn collect_reload_command_timeout_dispatch_console_messages(
    dispatch: &SupervisorReloadCommandTimeoutDispatch,
    out: &mut Vec<String>,
) {
    collect_service_transition_console_message(&dispatch.timeout.service_transition, out);
}

fn collect_terminal_dispatch_console_messages(
    dispatch: &SupervisorTerminalDispatch,
    out: &mut Vec<String>,
) {
    collect_service_transitions_console_messages(&dispatch.terminal.service_transitions, out);
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
    collect_restart_start_dispatches_console_messages(&dispatch.restart_start_dispatches, out);
    if dispatch.critical_reboot.is_some()
        && let Some(service) = dispatch.terminal.job_event.service.as_deref()
    {
        push_critical_service_failure(out, service, "service main exited");
    }
}

fn collect_pre_start_hook_terminal_console_messages(
    dispatch: &SupervisorPreStartHookTerminalDispatch,
    out: &mut Vec<String>,
) {
    collect_service_transitions_console_messages(&dispatch.terminal.service_transitions, out);
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

fn collect_post_start_hook_terminal_dispatch_console_messages(
    dispatch: &SupervisorPostStartHookTerminalDispatch,
    out: &mut Vec<String>,
) {
    collect_post_start_hook_terminal_console_messages(&dispatch.terminal, out);
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

fn collect_watchdog_timeout_console_messages(
    dispatch: &SupervisorWatchdogTimeoutDispatch,
    out: &mut Vec<String>,
) {
    collect_service_transitions_console_messages(&dispatch.service_transitions, out);
    if dispatch.critical_reboot.is_some() {
        push_critical_service_failure(out, &dispatch.service, "watchdog timeout");
    }
}
