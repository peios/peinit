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
}
