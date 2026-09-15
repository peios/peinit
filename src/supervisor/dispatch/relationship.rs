#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorOnFailureLoopSuppressionReason {
    Cycle,
    MaxDepth { max_depth: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorOnFailureLoopSuppressedDispatch {
    pub failed_service: String,
    pub attempted_handler: String,
    pub chain: Vec<String>,
    pub reason: SupervisorOnFailureLoopSuppressionReason,
}

/// Why a service held for a restart (§6.1) will not be coming back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorHeldRestartAbandonReason {
    /// Stopped by an operator while in Backoff.
    Stopped,
    /// Its definition was withdrawn while in Backoff, which discards the
    /// entry: there is nothing left to restart from.
    Withdrawn,
    /// It reached Failed by a route with no start operation through the
    /// graph — peinit could not execute the due restart.
    Failed,
}

/// How a hold on a service in Backoff was settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorHeldRestartOutcome {
    /// The service reached a dependent-satisfying state: the dependents
    /// held for it are released.
    Released,
    /// The service will not be coming back: the hard dependents held for
    /// it fail with `DependencyFailure`, the soft ones proceed.
    Abandoned(SupervisorHeldRestartAbandonReason),
}

/// A hold on a service in Backoff settled by the service's *state* rather
/// than by an operation of its own through the graph (PEI-821).
///
/// A relaunch that reaches Active, or fails for good, settles the hold on
/// the way through the ordinary terminal paths and is reported by them. The
/// routes here have no such operation — a `stop` of a service in Backoff is
/// synchronous, a withdrawn definition discards the entry, a restart peinit
/// could not execute fails the service directly — so this dispatch carries
/// the evidence: which dependents failed or were released, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorHeldRestartSettlementDispatch {
    pub target: String,
    pub outcome: SupervisorHeldRestartOutcome,
    pub operation_events: Vec<crate::operation::store::OperationEvent>,
    pub service_transitions: Vec<crate::service::ServiceTableTransition>,
    pub graph_events: Vec<crate::execution::graph::GraphExecutionEvent>,
    pub start_dispatches: Vec<crate::execution::start::StartExecutionDispatch>,
}
