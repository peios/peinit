use crate::execution::start::StartExecutionDispatch;
use crate::service::ServiceTableTransition;
use crate::service::runtime::ServiceState;
use crate::supervisor::dispatch::SupervisorOnFailureLoopSuppressedDispatch;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::super::model::OnFailureChain;
use super::super::start::dispatch_on_failure_start;

pub(super) fn apply_on_failure_starts(
    work: &mut SupervisorWork,
    transitions: &[ServiceTableTransition],
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    let mut dispatches = Vec::new();
    for transition in transitions {
        let event = &transition.event;
        if event.to != ServiceState::Failed || !event.cause.triggers_on_failure() {
            continue;
        }
        if critical_budget_reboot_suppresses_on_failure(work, &event.service) {
            continue;
        }
        let Some(target) = on_failure_target(work, &event.service) else {
            continue;
        };
        if target == event.service {
            continue;
        }

        let chain = work
            .relationships
            .take_on_failure_chain(&event.service)
            .unwrap_or_else(OnFailureChain::root);
        let chain = match chain.with_handler(&target) {
            Ok(chain) => chain,
            Err(reason) => {
                let chain = chain.path_with_attempted_handler(&target);
                work.relationships
                    .record_audit_event(SupervisorOnFailureLoopSuppressedDispatch {
                        failed_service: event.service.clone(),
                        attempted_handler: target,
                        chain,
                        reason,
                    });
                continue;
            }
        };
        dispatches.extend(dispatch_on_failure_start(
            work,
            &target,
            chain,
            observed_at_ns,
            max_parallel_starts,
        )?);
    }
    Ok(dispatches)
}

pub(super) fn clear_finished_on_failure_chains(
    work: &mut SupervisorWork,
    transitions: &[ServiceTableTransition],
) {
    for transition in transitions {
        if matches!(
            transition.event.to,
            ServiceState::Completed
                | ServiceState::Inactive
                | ServiceState::Skipped
                | ServiceState::Abandoned
        ) {
            work.relationships
                .clear_on_failure_chain(&transition.event.service);
        }
    }
}

fn on_failure_target(work: &SupervisorWork, service: &str) -> Option<String> {
    work.services
        .definition(service)
        .and_then(|definition| definition.on_failure.clone())
}

/// The immediate reboot owed to a Critical service takes precedence over
/// `OnFailure`, so the handler must not be started as well (§5.2, §5.3).
///
/// The question is deliberately delegated rather than restated here. This used
/// to key on the cause and `ErrorControl` alone, which is the *inputs* to the
/// reboot decision rather than the decision — so on every path that failed to
/// raise the reboot, the handler was suppressed on the strength of an
/// escalation that never happened, and a Critical service got neither. Asking
/// the one predicate means the two cannot disagree again.
fn critical_budget_reboot_suppresses_on_failure(work: &SupervisorWork, service: &str) -> bool {
    crate::supervisor::critical_budget::critical_budget_reboot_owed(&work.services, service)
}
