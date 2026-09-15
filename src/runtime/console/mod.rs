mod policy;
mod turn;
mod work;

#[cfg(test)]
mod tests;

// Gated to match its only non-test consumer, `runtime::linux`. The module
// itself also builds under `test`, so without this the re-export is dead
// under a default `cargo test` and warns.
#[cfg(feature = "peios-boundary")]
pub(crate) use policy::QuietPolicy;

use crate::console_style::ConsoleTag;
use crate::runtime::{RuntimeCalendarTimerTurn, RuntimeShutdownEventTurn, RuntimeWorkPumpTurn};
use crate::service::ServiceTableTransition;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::shutdown::ShutdownFinalizationState;

pub(crate) fn collect_runtime_loop_console_messages(
    pre_work: &RuntimeWorkPumpTurn,
    event_turns: &[RuntimeShutdownEventTurn],
    post_work: &RuntimeWorkPumpTurn,
    calendar_turns: &[(i32, RuntimeCalendarTimerTurn)],
    out: &mut Vec<ConsoleMessage>,
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

/// How much a console message is worth interrupting for.
///
/// Console output is gated by `peios.quiet` and by terminal ownership (see
/// `QuietPolicy`), and those two gates do not treat all messages alike — so the
/// severity has to travel with the message rather than being inferred at the
/// sink from its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConsoleSeverity {
    /// Ordinary progress. The first thing either gate suppresses.
    Status,
    /// Something went wrong. Loud enough to override a requested blackout —
    /// silence was a preference, and this is news — but not to justify writing
    /// into a terminal another process owns.
    Error,
    /// The operator must see this or lose the machine: dropping to recovery,
    /// halting with no shell, a Critical failure about to force a reboot.
    /// Worth one corrupted line of somebody else's session.
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsoleMessage {
    pub text: String,
    pub severity: ConsoleSeverity,
    /// How the line is tagged when it reaches the console. Carried with the
    /// message rather than inferred at the sink from its text, for the same
    /// reason severity is: the producer knows what happened and the sink does
    /// not, and matching on prose is how a rename silently turns every `OK`
    /// into a blank.
    pub tag: ConsoleTag,
}

impl ConsoleMessage {
    /// Progress with no outcome yet, and the unnamed default. See
    /// [`push_message`].
    pub fn status(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: ConsoleSeverity::Status,
            tag: ConsoleTag::None,
        }
    }

    /// Something worked. Status severity — a success is ordinary progress as
    /// far as `peios.quiet` is concerned; the tag is presentation, not news.
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: ConsoleSeverity::Status,
            tag: ConsoleTag::Ok,
        }
    }

    /// Deliberately not done. Also Status: a skip is the mechanism working.
    pub fn skip(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: ConsoleSeverity::Status,
            tag: ConsoleTag::Skip,
        }
    }

    /// Wrong, but the boot continues. Error severity, so it survives a
    /// requested blackout.
    pub fn warn(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: ConsoleSeverity::Error,
            tag: ConsoleTag::Warn,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: ConsoleSeverity::Error,
            tag: ConsoleTag::Failed,
        }
    }

    pub fn critical(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: ConsoleSeverity::Critical,
            tag: ConsoleTag::Crit,
        }
    }
}

pub(super) fn collect_start_dispatches_console_messages(
    dispatches: &[crate::execution::start::StartExecutionDispatch],
    out: &mut Vec<ConsoleMessage>,
) {
    for dispatch in dispatches {
        if let Some(cleared) = &dispatch.cleared_skipped {
            collect_service_transition_console_message(cleared, out);
        }
        collect_service_transition_console_message(&dispatch.service_transition, out);
    }
}

pub(super) fn collect_restart_start_dispatches_console_messages(
    dispatches: &[crate::execution::start::RestartStartExecutionDispatch],
    out: &mut Vec<ConsoleMessage>,
) {
    for dispatch in dispatches {
        collect_service_transition_console_message(&dispatch.service_transition, out);
    }
}

pub(super) fn collect_start_failure_console_messages(
    dispatch: &crate::execution::failure::StartFailureDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    collect_service_transitions_console_messages(&dispatch.service_transitions, out);
}

pub(super) fn collect_service_transitions_console_messages(
    transitions: &[ServiceTableTransition],
    out: &mut Vec<ConsoleMessage>,
) {
    for transition in transitions {
        collect_service_transition_console_message(transition, out);
    }
}

pub(super) fn collect_service_transition_console_message(
    transition: &ServiceTableTransition,
    out: &mut Vec<ConsoleMessage>,
) {
    let event = &transition.event;
    match event.to {
        ServiceState::Failed => push_error(
            out,
            format!(
                "peinit: service {} failed: {:?}\n",
                event.service, event.cause
            ),
        ),
        ServiceState::Skipped if skipped_for_boot_problem(event.cause) => push_skip(
            out,
            format!(
                "peinit: service {} skipped: {:?}\n",
                event.service, event.cause
            ),
        ),
        ServiceState::Abandoned => push_error(
            out,
            format!("peinit: service {} abandoned\n", event.service),
        ),
        _ => {}
    }
}

/// What the console says about an internal error contained to one service
/// (PEI-1125): a `[FAILED]` line naming the service, the step and the error,
/// then the transition line a failure gets anyway, then whatever the
/// failure's reactions started. Loud on purpose — this replaces a `[ CRIT ]`
/// recovery entry, and a quiet downgrade would hide the fault it names.
pub(crate) fn push_internal_error_messages(
    out: &mut Vec<ConsoleMessage>,
    dispatch: &crate::supervisor::SupervisorInternalErrorDispatch,
) {
    push_error(out, format!("{}\n", dispatch.message()));
    if let Some(transition) = &dispatch.service_transition {
        collect_service_transition_console_message(transition, out);
    }
    collect_start_dispatches_console_messages(&dispatch.start_dispatches, out);
}

pub(super) fn push_service_started(
    out: &mut Vec<ConsoleMessage>,
    job_event: &crate::job::JobEvent,
) {
    if let Some(service) = job_event.service.as_deref() {
        push_ok(out, format!("peinit: service {service} started\n"));
    }
}

pub(super) fn push_service_launch_failed(
    out: &mut Vec<ConsoleMessage>,
    job_event: &crate::job::JobEvent,
) {
    let Some(service) = job_event.service.as_deref() else {
        return;
    };
    // Report WHY, not just that. A launch fails before exec — a token that could
    // not be materialised, a runtime directory that could not be created or
    // secured, a cgroup that could not be set up — and the bare name leaves an
    // operator with a service that "failed to launch" and no way to tell those
    // apart without a debugger. The cause is already on the job event; Phase 1's
    // registryd path has always printed it, and there is no reason the runtime's
    // services deserve less.
    match job_event.failure_cause.as_deref() {
        Some(cause) => push_error(
            out,
            format!("peinit: service {service} failed to launch: {cause}\n"),
        ),
        None => push_error(out, format!("peinit: service {service} failed to launch\n")),
    }
}

pub(super) fn push_critical_service_failure(
    out: &mut Vec<ConsoleMessage>,
    service: &str,
    reason: &str,
) {
    push_critical(
        out,
        format!("peinit: critical service {service} failed: {reason}\n"),
    );
}

/// The machine is rebooting because a Critical service ran out of restart
/// budget. Critical, and phrased so the reason is on the same line as the
/// consequence: this is the last thing the operator sees before the reboot.
/// Preceded by what spent the budget, where the path that saw it said.
fn push_critical_budget_reboot_announcement(
    out: &mut Vec<ConsoleMessage>,
    service: &str,
    trigger: crate::supervisor::CriticalRebootTrigger,
) {
    if let Some(reason) = trigger.console_reason() {
        push_critical_service_failure(out, service, reason);
    }
    push_critical(
        out,
        format!("peinit: critical service {service} exhausted its restart budget; rebooting\n"),
    );
}

/// A Critical reboot raised inline, with its outcome. Only ever seen in
/// full when `reboot(2)` returned: the operator then needs to know the
/// machine is still up and retrying, not rebooting (PEI-1087).
pub(crate) fn push_critical_budget_reboot_messages(
    out: &mut Vec<ConsoleMessage>,
    dispatch: &crate::supervisor::SupervisorCriticalBudgetRebootDispatch,
) {
    push_critical_budget_reboot_announcement(out, &dispatch.service, dispatch.trigger);
    collect_shutdown_finalization_state_console_message(&dispatch.finalization.finalization, out);
}

/// What the runtime says before the final action it is about to take.
///
/// Written and flushed ahead of the action, because the action does not
/// return on success and nothing said afterwards is heard (PEI-827). The
/// Critical reboot names the service and what spent its budget; a
/// shutdown's final action is the "finalizing" line of the graceful
/// sequence. What either action reports afterwards, if it returns, is
/// [`collect_shutdown_finalization_turn_console_messages`].
pub(crate) fn push_pending_shutdown_finalization_messages(
    out: &mut Vec<ConsoleMessage>,
    pending: &crate::runtime::RuntimePendingShutdownFinalization,
) {
    match pending {
        crate::runtime::RuntimePendingShutdownFinalization::CriticalBudgetReboot(owed) => {
            push_critical_budget_reboot_announcement(out, &owed.service, owed.trigger);
        }
        crate::runtime::RuntimePendingShutdownFinalization::FinalAction => {
            push_message(out, "peinit: shutdown finalizing\n");
        }
    }
}

/// What a final action that returned has to report: the seed write that
/// failed on the way, and the failed-shutdown state the machine is now in.
pub(crate) fn collect_shutdown_finalization_turn_console_messages(
    finalization: &crate::runtime::RuntimeShutdownFinalizationTurn,
    out: &mut Vec<ConsoleMessage>,
) {
    if let Some(reboot) = &finalization.critical_budget_reboot {
        collect_shutdown_finalization_state_console_message(&reboot.finalization.finalization, out);
    }
    if let Some(dispatch) = &finalization.finalization {
        collect_shutdown_finalization_result_console_messages(dispatch, out);
    }
}

/// The outcome of a final action: everything
/// `collect_shutdown_finalization_dispatch_console_messages` says except
/// the "finalizing" line, which the runtime has already said.
pub(super) fn collect_shutdown_finalization_result_console_messages(
    dispatch: &crate::supervisor::SupervisorShutdownFinalizationDispatch,
    out: &mut Vec<ConsoleMessage>,
) {
    if let crate::shutdown::CleanupActionResult::Failed(message) = &dispatch.report.random_seed {
        push_message(
            out,
            format!("peinit warning: shutdown random seed save failed: {message}\n"),
        );
    }
    collect_shutdown_finalization_state_console_message(&dispatch.finalization, out);
}

pub(super) fn collect_shutdown_finalization_state_console_message(
    finalization: &ShutdownFinalizationState,
    out: &mut Vec<ConsoleMessage>,
) {
    match finalization {
        ShutdownFinalizationState::WaitingForServices => {}
        ShutdownFinalizationState::Ready => {
            push_message(out, "peinit: shutdown ready to finalize\n")
        }
        ShutdownFinalizationState::Failed { message, .. } => push_warn(
            out,
            format!("peinit: shutdown final action failed: {message}\n"),
        ),
        ShutdownFinalizationState::Completed => push_ok(out, "peinit: shutdown completed\n"),
    }
}

/// Ordinary progress. Deliberately the unnamed default: the great majority of
/// console output is status, and requiring every site to state a severity would
/// make the few that are not status harder to spot rather than easier.
pub(super) fn push_message(out: &mut Vec<ConsoleMessage>, message: impl Into<String>) {
    out.push(ConsoleMessage::status(message));
}

/// Something worked. Same severity as [`push_message`]; the difference is the
/// tag the operator sees.
pub(super) fn push_ok(out: &mut Vec<ConsoleMessage>, message: impl Into<String>) {
    out.push(ConsoleMessage::ok(message));
}

/// Deliberately not done.
pub(super) fn push_skip(out: &mut Vec<ConsoleMessage>, message: impl Into<String>) {
    out.push(ConsoleMessage::skip(message));
}

/// Wrong, but the boot continues.
pub(super) fn push_warn(out: &mut Vec<ConsoleMessage>, message: impl Into<String>) {
    out.push(ConsoleMessage::warn(message));
}

/// Something went wrong. Overrides a requested blackout; does not override
/// another process owning the terminal.
pub(super) fn push_error(out: &mut Vec<ConsoleMessage>, message: impl Into<String>) {
    out.push(ConsoleMessage::error(message));
}

/// The operator must see this or lose the machine. Overrides both gates.
pub(super) fn push_critical(out: &mut Vec<ConsoleMessage>, message: impl Into<String>) {
    out.push(ConsoleMessage::critical(message));
}

/// `TtyUnavailable` is deliberately absent. It is not a boot problem —
/// a service queued on a busy console is the mechanism working — and printing
/// it would write onto the very terminal whose new owner is at that moment
/// drawing on it.
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
