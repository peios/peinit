use crate::execution::graph::GraphExecutionStore;
use crate::ids::JobIdAllocator;
use crate::job::JobStore;
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;

use super::super::model::{PreStartCheckCompletionContext, StartExecutionContext};
use super::super::store::StartExecutionStore;

// A filesystem-check completion can release graph work, create jobs, or fail a
// start. Keep the affected stores together so the completion commits atomically.
pub(super) struct PreStartCheckTransaction {
    pub(super) services: ServiceTable,
    pub(super) operations: OperationStore,
    pub(super) graph: GraphExecutionStore,
    pub(super) jobs: JobStore,
    pub(super) job_ids: JobIdAllocator,
    pub(super) start_store: StartExecutionStore,
}

impl PreStartCheckTransaction {
    pub(super) fn from_completion_context(
        context: &mut PreStartCheckCompletionContext<'_>,
    ) -> Self {
        Self {
            services: (*context.services).clone(),
            operations: (*context.operations).clone(),
            graph: (*context.graph).clone(),
            jobs: (*context.jobs).clone(),
            job_ids: (*context.job_ids).clone(),
            start_store: (*context.start_store).clone(),
        }
    }

    pub(super) fn commit_to_completion_context(
        self,
        context: &mut PreStartCheckCompletionContext<'_>,
    ) {
        *context.services = self.services;
        *context.operations = self.operations;
        *context.graph = self.graph;
        *context.jobs = self.jobs;
        *context.job_ids = self.job_ids;
        *context.start_store = self.start_store;
    }

    pub(super) fn from_start_context<P>(context: &mut StartExecutionContext<'_, P>) -> Self
    where
        P: crate::boundary::ProcessController + ?Sized,
    {
        Self {
            services: (*context.services).clone(),
            operations: (*context.operations).clone(),
            graph: (*context.graph).clone(),
            jobs: (*context.jobs).clone(),
            job_ids: (*context.job_ids).clone(),
            start_store: (*context.start_store).clone(),
        }
    }

    pub(super) fn commit_to_start_context<P>(self, context: &mut StartExecutionContext<'_, P>)
    where
        P: crate::boundary::ProcessController + ?Sized,
    {
        *context.services = self.services;
        *context.operations = self.operations;
        *context.graph = self.graph;
        *context.jobs = self.jobs;
        *context.job_ids = self.job_ids;
        *context.start_store = self.start_store;
    }
}
