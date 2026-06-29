use crate::execution::start::StartExecutionDispatch;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::super::conflict::{blocker_still_present, gate_start_context};

pub(super) fn release_unblocked_conflict_contexts(
    work: &mut SupervisorWork,
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
    let pending = work.relationships.pending_conflict_contexts();
    let mut dispatches = Vec::new();
    for context in pending {
        if context
            .blockers
            .iter()
            .any(|blocker| blocker_still_present(&work.services, blocker))
        {
            continue;
        }
        let Some(context) = work
            .relationships
            .remove_conflict_context(context.context_id)
        else {
            continue;
        };
        dispatches.extend(gate_start_context(
            work,
            context.context_id,
            context.start_services,
            observed_at_ns,
            max_parallel_starts,
        )?);
    }
    Ok(dispatches)
}
