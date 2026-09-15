use crate::runtime::RuntimeWorkPumpTurn;
use crate::runtime::console::ConsoleMessage;
use crate::supervisor::{SupervisorHeldRestartAbandonReason, SupervisorHeldRestartOutcome};

use super::{
    collect_start_dispatches_console_messages, collect_start_failure_console_messages,
    push_service_launch_failed, push_service_started,
};

fn abandon_reason_text(reason: SupervisorHeldRestartAbandonReason) -> &'static str {
    match reason {
        SupervisorHeldRestartAbandonReason::Stopped => "was stopped in Backoff",
        SupervisorHeldRestartAbandonReason::Withdrawn => "was withdrawn in Backoff",
        SupervisorHeldRestartAbandonReason::Failed => "failed in Backoff",
    }
}

pub(super) fn collect_runtime_work_pump_console_messages(
    turn: &RuntimeWorkPumpTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    for dispatch in &turn.control_operation_failures {
        // The service kept its state, so no transition says anything went
        // wrong. Without this line an operator whose command failed this way
        // has a Failed operation to find and nothing on the console.
        super::push_error(
            out,
            format!(
                "peinit: service {}: {:?} operation {} failed before it began: {:?}\n",
                dispatch.service, dispatch.operation_type, dispatch.operation_id, dispatch.error
            ),
        );
    }
    for settlement in &turn.held_restart_settlements {
        // The dependents' own transition lines follow; this one says why
        // services that never started are now Failed — or released — when
        // the target's route out of Backoff had no line of its own for it.
        let held = settlement.graph_events.len().saturating_sub(1);
        match settlement.outcome {
            SupervisorHeldRestartOutcome::Released => super::push_ok(
                out,
                format!(
                    "peinit: service {} is back: {held} dependent(s) held for its restart released\n",
                    settlement.target
                ),
            ),
            SupervisorHeldRestartOutcome::Abandoned(reason) => super::push_error(
                out,
                format!(
                    "peinit: service {} {}: {} dependent(s) held for its restart failed\n",
                    settlement.target,
                    abandon_reason_text(reason),
                    settlement.service_transitions.len()
                ),
            ),
        }
        super::collect_service_transitions_console_messages(&settlement.service_transitions, out);
        collect_start_dispatches_console_messages(&settlement.start_dispatches, out);
    }
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
