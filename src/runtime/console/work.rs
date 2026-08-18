use crate::runtime::RuntimeWorkPumpTurn;
use crate::runtime::console::ConsoleMessage;

use super::{
    collect_start_dispatches_console_messages, collect_start_failure_console_messages,
    push_service_launch_failed, push_service_started,
};

pub(super) fn collect_runtime_work_pump_console_messages(
    turn: &RuntimeWorkPumpTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    for dispatch in &turn.service_launches {
        push_service_started(out, &dispatch.started.job_event);
        collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
    }
    for dispatch in &turn.service_launch_failures {
        push_service_launch_failed(out, &dispatch.job_event);
        collect_start_failure_console_messages(&dispatch.failure, out);
        collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
    }
    for dispatch in &turn.start_hook_launch_failures {
        collect_start_failure_console_messages(&dispatch.failure, out);
        collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
    }
    for dispatch in &turn.post_hook_launch_failures {
        super::turn::collect_post_start_hook_terminal_console_messages(&dispatch.terminal, out);
        collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
    }
    for dispatch in &turn.health_check_launch_failures {
        super::turn::collect_health_check_terminal_console_messages(&dispatch.terminal, out);
    }
}
