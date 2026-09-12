use std::collections::{BTreeSet, VecDeque};

use crate::execution::graph::{
    GraphContextId, GraphExecutionEvent, ReadyGraphOperation, ReadyGraphOperationAction,
    probe_level,
};
use crate::execution::start::{
    GraphPreStartCheckOutcome, PrecheckedReadyStartOutcome, StartExecutionDispatch,
    StartExecutionRequest, begin_graph_pre_start_check, begin_prechecked_ready_start,
};
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
            let services = &self.services;
            let ready = self
                .graph
                .release_ready(context_id, max_parallel_starts, &|target, level| {
                    probe_level(services, target, level)
                })
                .map_err(SupervisorError::Graph)?;

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
