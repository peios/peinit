//! What it means for a start to be *held*.
//!
//! A member the scheduler has not yet released is waiting on something,
//! and the operation lifetime of §8.2 runs against that wait — with one
//! exception, which §7.5 names: a start held on a fact that has no clock
//! of its own does not time out. That is the declared semantics, not a
//! hang; the condition was never met, so the start never happened.
//!
//! The distinction is drawn edge by edge. A readiness level is such a fact:
//! nothing bounds when a publisher will publish it. So is a target in
//! Backoff: the restart policy owns that clock, and the dependents wait on
//! its outcome (§6.1). A target that is *starting* is not: its own
//! `StartTimeout` bounds the wait, and a dependent whose lifetime is
//! shorter fails on its own clock first — which is what the lostdep
//! anchors promise. So a start is exempt only while every unsatisfied
//! edge it waits on is undecided in that sense, transitively: a member
//! held only on such edges is itself such a fact to whatever waits on it.
//! Everything else keeps its lifetime, queue time included (PEI-830).

use crate::ids::OperationId;

use super::model::{GraphDependency, GraphExecutionContext, GraphMemberStatus};
use super::store::GraphExecutionStore;

/// What an unsatisfied edge is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeWait {
    /// Nothing: the edge is settled, or its target is gone for good.
    Settled,
    /// A fact with no clock: a level not yet published, a target in
    /// Backoff, or a target itself held only on such facts.
    Undecided,
    /// A target with a clock of its own: starting, checking, or queued.
    Clocked,
}

impl GraphExecutionContext {
    /// Whether `service`'s start is held only on facts with no clock of
    /// their own, so that its operation lifetime does not run.
    pub fn waits_without_clock(&self, service: &str) -> bool {
        self.waits_without_clock_visiting(service, &mut Vec::new())
    }

    fn waits_without_clock_visiting<'a>(
        &'a self,
        service: &'a str,
        visiting: &mut Vec<&'a str>,
    ) -> bool {
        let Some(member) = self.members.get(service) else {
            return false;
        };
        if member.status != GraphMemberStatus::WaitingForDependencies {
            return false;
        }
        if visiting.contains(&service) {
            // A cycle would have been rejected at validation; treat one
            // as clocked rather than looping.
            return false;
        }
        visiting.push(service);
        let mut undecided = 0;
        let mut clocked = false;
        for edge in self
            .dependencies
            .iter()
            .filter(|edge| edge.dependent == service)
        {
            match self.edge_wait(edge, visiting) {
                EdgeWait::Settled => {}
                EdgeWait::Undecided => undecided += 1,
                EdgeWait::Clocked => {
                    clocked = true;
                    break;
                }
            }
        }
        visiting.pop();
        !clocked && undecided > 0
    }

    fn edge_wait<'a>(&'a self, edge: &'a GraphDependency, visiting: &mut Vec<&'a str>) -> EdgeWait {
        let target = self.members.get(&edge.target);
        if edge.level.is_some() {
            // The level itself is never clocked. What can be is the
            // target's own start, when the target is a member still
            // running it.
            return match target.map(|member| member.status) {
                None | Some(GraphMemberStatus::Satisfied | GraphMemberStatus::AwaitingRestart) => {
                    EdgeWait::Undecided
                }
                Some(GraphMemberStatus::Failed | GraphMemberStatus::Pruned) => EdgeWait::Settled,
                Some(GraphMemberStatus::WaitingForDependencies)
                    if self.waits_without_clock_visiting(&edge.target, visiting) =>
                {
                    EdgeWait::Undecided
                }
                Some(_) => EdgeWait::Clocked,
            };
        }
        match target.map(|member| member.status) {
            // A level-less edge always has a member target; a missing one
            // has nothing to wait on.
            None
            | Some(
                GraphMemberStatus::Satisfied
                | GraphMemberStatus::Failed
                | GraphMemberStatus::Pruned,
            ) => EdgeWait::Settled,
            Some(GraphMemberStatus::AwaitingRestart) => EdgeWait::Undecided,
            Some(GraphMemberStatus::WaitingForDependencies)
                if self.waits_without_clock_visiting(&edge.target, visiting) =>
            {
                EdgeWait::Undecided
            }
            Some(_) => EdgeWait::Clocked,
        }
    }
}

impl GraphExecutionContext {
    /// Record, on every member still waiting, whether it is held only on
    /// facts with no clock. Read back when the member is released, so the
    /// release knows the lifetime was stopped and starts it over. Run on
    /// every release pass and whenever a hold begins.
    pub(super) fn note_holds_without_clock(&mut self) {
        let holds = self
            .members
            .values()
            .filter(|member| member.status == GraphMemberStatus::WaitingForDependencies)
            .map(|member| {
                (
                    member.service.clone(),
                    self.waits_without_clock(&member.service),
                )
            })
            .collect::<Vec<_>>();
        for (service, held_without_clock) in holds {
            if let Some(member) = self.members.get_mut(&service) {
                member.held_without_clock = held_without_clock;
            }
        }
    }
}

impl GraphExecutionStore {
    /// Whether `operation_id` is a start held on facts with no clock of
    /// their own, in some live context — the one case in which its
    /// lifetime does not run.
    pub fn is_operation_held(&self, operation_id: OperationId) -> bool {
        self.associated_contexts(operation_id)
            .into_iter()
            .filter_map(|context_id| self.contexts.get(&context_id))
            .any(|context| {
                context
                    .member_for_operation(operation_id)
                    .is_some_and(|member| context.waits_without_clock(&member.service))
            })
    }
}
