use crate::runtime::console::ConsoleMessage;
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
    push_critical_service_failure, push_error,
};

pub(super) fn collect_child_reap_turn_console_messages(
    turn: &SupervisorChildReapTurn,
    out: &mut Vec<ConsoleMessage>,
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
        SupervisorChildReapDispatch::ReloadCommand(_)
        | SupervisorChildReapDispatch::Submitted(_) => {}
    }
}

pub(super) fn collect_lifecycle_deadline_dispatch_console_messages(
    dispatch: &SupervisorLifecycleDeadlineDispatch,
    out: &mut Vec<ConsoleMessage>,
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
    // A leak means something underneath the service stopped answering the
    // kernel, and no amount of restarting the service fixes it. The service
    // itself carries on, so without this line nothing on the console says the
    // machine is now missing a cgroup it can never reclaim.
    for leak in &dispatch.cgroup_leaks {
        crate::runtime::console::push_error(
            out,
            format!(
                "peinit: service {} leaked its {} cgroup {}; underlying process is not responding to the kernel\n",
                leak.service,
                leaked_cgroup_kind_console(leak.kind),
                leak.path,
            ),
        );
    }
    for settle in &dispatch.boot_settles {
        // Say when the wait was cut short, because it changes what the operator
        // is looking at: a prompt started on a timeout may still be written
        // over by whatever is still moving.
        if settle.timed_out && !settle.started.is_empty() {
            crate::runtime::console::push_message(
                out,
                "peinit: boot did not settle in time; starting deferred service(s) anyway\n"
                    .to_string(),
            );
        }
        for start in &settle.started {
            crate::runtime::console::collect_start_dispatches_console_messages(
                &start.lifecycle.start_dispatches,
                out,
            );
        }
        for failure in &settle.failed {
            crate::runtime::console::push_error(
                out,
                format!(
                    "peinit: deferred service {} could not be started: {}\n",
                    failure.service, failure.error
                ),
            );
        }
    }
}

pub(super) fn collect_post_start_hook_terminal_console_messages(
    dispatch: &crate::execution::start::PostStartHookTerminalDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    collect_service_transitions_console_messages(&dispatch.service_transitions, out);
}

pub(super) fn collect_health_check_terminal_console_messages(
    dispatch: &SupervisorHealthCheckTerminalDispatch,
    out: &mut Vec<ConsoleMessage>,
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
    out: &mut Vec<ConsoleMessage>,
) {
    collect_service_transitions_console_messages(&dispatch.timeout.service_transitions, out);
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

fn collect_reload_command_timeout_dispatch_console_messages(
    dispatch: &SupervisorReloadCommandTimeoutDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    collect_service_transition_console_message(&dispatch.timeout.service_transition, out);
}

fn collect_terminal_dispatch_console_messages(
    dispatch: &SupervisorTerminalDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    push_late_service_exit(&dispatch.terminal, out);
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
    out: &mut Vec<ConsoleMessage>,
) {
    collect_service_transitions_console_messages(&dispatch.terminal.service_transitions, out);
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

fn collect_post_start_hook_terminal_dispatch_console_messages(
    dispatch: &SupervisorPostStartHookTerminalDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    collect_post_start_hook_terminal_console_messages(&dispatch.terminal, out);
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

fn collect_watchdog_timeout_console_messages(
    dispatch: &SupervisorWatchdogTimeoutDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    collect_service_transitions_console_messages(&dispatch.service_transitions, out);
    if dispatch.critical_reboot.is_some() {
        push_critical_service_failure(out, &dispatch.service, "watchdog timeout");
    }
}

/// Report a main process that exited after its service had stopped expecting
/// one.
///
/// There is no transition to report — that is the point of a late exit — so
/// without this the event leaves no trace at all, and a service stuck in a
/// stale state would be a silent mystery. It is an error rather than status
/// because every route to it is something having gone wrong earlier: the
/// service was abandoned as unkillable, or it left a running process behind
/// when it failed.
fn push_late_service_exit(
    dispatch: &crate::execution::job_terminal::ServiceMainJobTerminalDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    let Some(state) = dispatch.late_exit else {
        return;
    };
    let Some(service) = dispatch.job_event.service.as_deref() else {
        return;
    };
    push_error(
        out,
        format!("peinit: service {service} main process exited in state {state:?}; no action taken\n"),
    );
}

fn leaked_cgroup_kind_console(kind: crate::service::runtime::LeakedCgroupKind) -> &'static str {
    use crate::service::runtime::LeakedCgroupKind;
    match kind {
        LeakedCgroupKind::ServiceTree => "service_tree",
        LeakedCgroupKind::Health => "health",
        LeakedCgroupKind::Hooks => "hooks",
        LeakedCgroupKind::Helper => "helper",
    }
}
