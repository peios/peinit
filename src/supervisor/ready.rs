use std::collections::{BTreeSet, VecDeque};

use crate::execution::graph::{
    GraphContextId, GraphExecutionEvent, ReadyGraphOperation, ReadyGraphOperationAction,
    probe_level,
};
use crate::execution::start::{
    GraphPreStartCheckOutcome, PrecheckedReadyStartOutcome, StartExecutionDispatch,
    StartExecutionRequest, begin_graph_pre_start_check, begin_prechecked_ready_start,
};
use crate::operation::OperationState;
use crate::security::TokenSummary;

use super::state::SupervisorError;
use super::work::SupervisorWork;

impl SupervisorWork {
    pub fn release_for_context(
        &mut self,
        context_id: GraphContextId,
        max_parallel_starts: u32,
        observed_at_ns: u64,
    ) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
        let mut context_ids = VecDeque::from([context_id]);
        let mut dispatches = Vec::new();

        while let Some(context_id) = context_ids.pop_front() {
            // PEI-1380: a transitive hold can end while its operation remains
            // Pending on a newly clocked dependency. Capture the old hold before
            // release_ready refreshes it, not only the starts ready to dispatch.
            let held = self.graph.recorded_held_operations();
            let services = &self.services;
            let mut ready = self
                .graph
                .release_ready(context_id, max_parallel_starts, &|target, level| {
                    probe_level(services, target, level)
                })
                .map_err(SupervisorError::Graph)?;

            self.restart_exited_hold_lifetimes(held, &mut ready, observed_at_ns)?;

            for ready in ready {
                match ready.action {
                    ReadyGraphOperationAction::PreStartCheck => {
                        let request = self.start_request(ready, observed_at_ns)?;
                        let outcome = begin_graph_pre_start_check(
                            &mut self.services,
                            &mut self.operations,
                            &mut self.graph,
                            &mut self.start,
                            request,
                        )
                        .map_err(SupervisorError::Start)?;
                        match outcome {
                            GraphPreStartCheckOutcome::Passed(dispatch) => {
                                context_ids.extend(dispatch.graph_context_ids);
                            }
                            GraphPreStartCheckOutcome::Terminal(dispatch) => {
                                context_ids.extend(Self::event_context_ids(&dispatch.graph_events));
                            }
                            GraphPreStartCheckOutcome::CheckPending(_) => {}
                        }
                    }
                    ReadyGraphOperationAction::Start => {
                        let request = self.start_request(ready, observed_at_ns)?;
                        let outcome = begin_prechecked_ready_start(
                            &mut self.services,
                            &mut self.operations,
                            &mut self.graph,
                            &mut self.jobs,
                            &mut self.job_ids,
                            &mut self.start,
                            request,
                        )
                        .map_err(SupervisorError::Start)?;
                        match outcome {
                            PrecheckedReadyStartOutcome::Job(dispatch) => {
                                self.queue_start_dispatches(std::slice::from_ref(&dispatch));
                                dispatches.push(*dispatch);
                            }
                            PrecheckedReadyStartOutcome::Terminal(dispatch) => {
                                context_ids.extend(Self::event_context_ids(&dispatch.graph_events));
                            }
                        }
                    }
                }
            }
        }

        Ok(dispatches)
    }

    /// Reconcile operation-level hold exits before dispatch. Kept separate
    /// from release_ready because the graph owns neither clocks nor operations.
    fn restart_exited_hold_lifetimes(
        &mut self,
        held: BTreeSet<crate::ids::OperationId>,
        ready: &mut [ReadyGraphOperation],
        observed_at_ns: u64,
    ) -> Result<(), SupervisorError> {
        for operation_id in held {
            // A shared operation stays exempt while any context holds it.
            for operation in ready.iter_mut() {
                if operation.operation_id == operation_id {
                    // This coordinator uses aggregate holds, not the ready
                    // member's per-context history. Standalone callers keep
                    // the original low-level reset semantics.
                    operation.released_from_hold = false;
                }
            }
            if self.graph.is_operation_held(operation_id) {
                continue;
            }
            if self
                .operations
                .get(operation_id)
                .is_some_and(|operation| operation.state == OperationState::Pending)
            {
                self.operations
                    .restart_lifetime_clock(operation_id, observed_at_ns)
                    .map_err(|error| {
                        SupervisorError::Start(
                            crate::execution::start::StartExecutionError::OperationStore(error),
                        )
                    })?;
            }
            self.graph.clear_recorded_operation_hold(operation_id);
        }
        Ok(())
    }

    fn start_request(
        &self,
        ready: ReadyGraphOperation,
        observed_at_ns: u64,
    ) -> Result<StartExecutionRequest, SupervisorError> {
        let definition = self.services.definition(&ready.service).ok_or_else(|| {
            SupervisorError::MissingStartCredentials {
                service: ready.service.clone(),
            }
        })?;
        let resolved_identity = definition.identity.clone();
        Ok(StartExecutionRequest {
            ready,
            resolved_identity: resolved_identity.clone(),
            token_summary: TokenSummary::requested_identity(resolved_identity),
            started_at_ns: observed_at_ns,
        })
    }

    pub fn release_after_graph_events(
        &mut self,
        events: &[GraphExecutionEvent],
        max_parallel_starts: u32,
        observed_at_ns: u64,
    ) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
        let mut context_ids = Self::event_context_ids(events);
        let mut dispatches = Vec::new();
        for context_id in std::mem::take(&mut context_ids) {
            dispatches.extend(self.release_for_context(
                context_id,
                max_parallel_starts,
                observed_at_ns,
            )?);
        }
        Ok(dispatches)
    }

    /// Release any context held on `service`'s readiness level.
    ///
    /// Called when the answer a level edge would get has changed: the
    /// service published a `LEVEL=` (possibly the one a dependent wants),
    /// or it left a dependent-satisfying state (freeing `Wants` waiters,
    /// whose gate opens when nobody could publish the level any more).
    pub fn release_level_waiters_on(
        &mut self,
        service: &str,
        max_parallel_starts: u32,
        observed_at_ns: u64,
    ) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
        let context_ids = self.graph.contexts_with_level_dependency_on(service);
        self.release_after_graph_context_ids(&context_ids, max_parallel_starts, observed_at_ns)
    }

    pub fn release_after_graph_context_ids(
        &mut self,
        context_ids: &[GraphContextId],
        max_parallel_starts: u32,
        observed_at_ns: u64,
    ) -> Result<Vec<StartExecutionDispatch>, SupervisorError> {
        let mut unique = context_ids.iter().copied().collect::<BTreeSet<_>>();
        let mut dispatches = Vec::new();
        for context_id in std::mem::take(&mut unique) {
            dispatches.extend(self.release_for_context(
                context_id,
                max_parallel_starts,
                observed_at_ns,
            )?);
        }
        Ok(dispatches)
    }
}
