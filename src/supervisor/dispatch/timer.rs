use crate::control::lifecycle::OnDemandStartDispatch;
use crate::execution::graph::GraphContextId;
use crate::execution::start::StartExecutionDispatch;
use crate::service::runtime::ServiceState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorTimerDispatch {
    pub service: String,
    pub schedule: String,
    pub action: SupervisorTimerAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorTimerAction {
    Start {
        requested_operation_id: crate::ids::OperationId,
        outcome: Box<OnDemandStartDispatch>,
        context_id: GraphContextId,
        start_dispatches: Vec<StartExecutionDispatch>,
    },
    PendingOneshot {
        newly_pending: bool,
    },
    SimpleNoop,
    StateNoop {
        state: ServiceState,
    },
    Disabled,
    /// The firing was ignored because a shutdown is in progress.
    ShutdownInProgress,
    /// The service is no longer in the table: its definition was deleted
    /// while it ran, and it was discarded when it stopped, which no reload
    /// followed to re-plan its timers (PEI-1234). The firing does nothing,
    /// and the runtime neither records it nor arms the timer again.
    ServiceGone,
}
