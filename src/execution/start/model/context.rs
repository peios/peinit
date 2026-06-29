use crate::boundary::ProcessController;
use crate::execution::graph::GraphExecutionStore;
use crate::ids::JobIdAllocator;
use crate::job::JobStore;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;

use super::super::store::StartExecutionStore;

pub struct StartExecutionContext<'a, P>
where
    P: ProcessController + ?Sized,
{
    pub services: &'a mut ServiceTable,
    pub operations: &'a mut OperationStore,
    pub graph: &'a mut GraphExecutionStore,
    pub jobs: &'a mut JobStore,
    pub job_ids: &'a mut JobIdAllocator,
    pub start_store: &'a mut StartExecutionStore,
    pub controller: &'a mut P,
}

pub struct PreStartCheckCompletionContext<'a> {
    pub services: &'a mut ServiceTable,
    pub operations: &'a mut OperationStore,
    pub graph: &'a mut GraphExecutionStore,
    pub jobs: &'a mut JobStore,
    pub job_ids: &'a mut JobIdAllocator,
    pub start_store: &'a mut StartExecutionStore,
}

pub struct StartReadyContext<'a> {
    pub services: &'a mut ServiceTable,
    pub operations: &'a mut OperationStore,
    pub graph: &'a mut GraphExecutionStore,
    pub jobs: &'a mut JobStore,
    pub job_ids: &'a mut JobIdAllocator,
    pub start_store: &'a mut StartExecutionStore,
}
