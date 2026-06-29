mod turn;
mod work;

#[cfg(test)]
mod tests;

use crate::runtime::{RuntimeCalendarTimerTurn, RuntimeShutdownEventTurn, RuntimeWorkPumpTurn};
use crate::service::ServiceTableTransition;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::shutdown::ShutdownFinalizationState;

pub(crate) fn collect_runtime_loop_console_messages(
    pre_work: &RuntimeWorkPumpTurn,
    event_turns: &[RuntimeShutdownEventTurn],
    post_work: &RuntimeWorkPumpTurn,
    calendar_turns: &[(i32, RuntimeCalendarTimerTurn)],
    out: &mut Vec<String>,
) {
    work::collect_runtime_work_pump_console_messages(pre_work, out);
    for event_turn in event_turns {
        turn::collect_runtime_shutdown_turn_console_messages(event_turn, out);
    }
    work::collect_runtime_work_pump_console_messages(post_work, out);
    for (_, calendar_turn) in calendar_turns {
        turn::collect_runtime_calendar_timer_console_messages(calendar_turn, out);
    }
}

pub(super) fn collect_start_dispatches_console_messages(
    dispatches: &[crate::execution::start::StartExecutionDispatch],
    out: &mut Vec<String>,
) {
    for dispatch in dispatches {
        collect_service_transition_console_message(&dispatch.service_transition, out);
    }
}

pub(super) fn collect_restart_start_dispatches_console_messages(
    dispatches: &[crate::execution::start::RestartStartExecutionDispatch],
    out: &mut Vec<String>,
) {
    for dispatch in dispatches {
        collect_service_transition_console_message(&dispatch.service_transition, out);
    }
}

pub(super) fn collect_start_failure_console_messages(
    dispatch: &crate::execution::failure::StartFailureDispatch,
    out: &mut Vec<String>,
) {
    collect_service_transitions_console_messages(&dispatch.service_transitions, out);
}

pub(super) fn collect_service_transitions_console_messages(
    transitions: &[ServiceTableTransition],
    out: &mut Vec<String>,
) {
    for transition in transitions {
        collect_service_transition_console_message(transition, out);
    }
}

pub(super) fn collect_service_transition_console_message(
    transition: &ServiceTableTransition,
    out: &mut Vec<String>,
) {
    let event = &transition.event;
    match event.to {
        ServiceState::Failed => push_message(
            out,
            format!(
                "peinit: service {} failed: {:?}\n",
                event.service, event.cause
            ),
        ),
        ServiceState::Skipped if skipped_for_boot_problem(event.cause) => push_message(
            out,
            format!(
                "peinit: service {} skipped: {:?}\n",
                event.service, event.cause
            ),
        ),
        ServiceState::Abandoned => push_message(
            out,
            format!("peinit: service {} abandoned\n", event.service),
        ),
        _ => {}
    }
}

pub(super) fn push_service_started(out: &mut Vec<String>, job_event: &crate::job::JobEvent) {
    if let Some(service) = job_event.service.as_deref() {
        push_message(out, format!("peinit: service {service} started\n"));
    }
}

pub(super) fn push_service_launch_failed(out: &mut Vec<String>, job_event: &crate::job::JobEvent) {
    if let Some(service) = job_event.service.as_deref() {
        push_message(out, format!("peinit: service {service} failed to launch\n"));
    }
}

pub(super) fn push_critical_service_failure(out: &mut Vec<String>, service: &str, reason: &str) {
    push_message(
        out,
        format!("peinit: critical service {service} failed: {reason}\n"),
    );
}

pub(super) fn collect_shutdown_finalization_state_console_message(
    finalization: &ShutdownFinalizationState,
    out: &mut Vec<String>,
) {
    match finalization {
        ShutdownFinalizationState::WaitingForServices => {}
        ShutdownFinalizationState::Ready => {
            push_message(out, "peinit: shutdown ready to finalize\n")
        }
        ShutdownFinalizationState::Failed { message, .. } => push_message(
            out,
            format!("peinit: shutdown final action failed: {message}\n"),
        ),
        ShutdownFinalizationState::Completed => push_message(out, "peinit: shutdown completed\n"),
    }
}

pub(super) fn push_message(out: &mut Vec<String>, message: impl Into<String>) {
    out.push(message.into());
}

fn skipped_for_boot_problem(cause: TransitionCause) -> bool {
    matches!(
        cause,
        TransitionCause::DependencyFailure
            | TransitionCause::CycleDetected
            | TransitionCause::ValidationError
            | TransitionCause::AssertionError
            | TransitionCause::ConditionSkipped
    )
}
