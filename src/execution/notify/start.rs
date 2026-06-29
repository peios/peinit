use crate::execution::start::{StartReadyContext, StartReadyRequest, complete_start_readiness};
use crate::service::{Readiness, ServiceType};

use super::error::unknown_service;
use super::field::NotifyFieldContext;
use super::model::NotifyApplyError;

pub(super) fn apply_start_ready(
    context: &mut NotifyFieldContext<'_>,
) -> Result<(), NotifyApplyError> {
    let definition = context
        .services
        .definition(&context.sender.service)
        .ok_or_else(|| unknown_service(&context.sender.service))?;
    if definition.service_type != ServiceType::Simple || definition.readiness != Readiness::Notify {
        return Ok(());
    }
    let operation_id =
        context
            .sender
            .operation_id
            .ok_or_else(|| NotifyApplyError::MissingStartOperation {
                service: context.sender.service.clone(),
                job_id: context.sender.job_id,
            })?;
    let readiness = complete_start_readiness(
        &mut StartReadyContext {
            services: &mut *context.services,
            operations: &mut *context.operations,
            graph: &mut *context.graph,
            jobs: &mut *context.jobs,
            job_ids: &mut *context.job_ids,
            start_store: &mut *context.start_store,
        },
        StartReadyRequest {
            service: context.sender.service.clone(),
            operation_id,
            job_id: context.sender.job_id,
            job_created_at_ns: context.sender.job_created_at_ns,
            activation_generation: context.sender.generation,
            cgroup_generation: context.sender.cgroup_generation,
            ready_at_ns: context.observed_at_ns,
            result: "notify readiness: READY=1".to_string(),
        },
    )
    .map_err(NotifyApplyError::Start)?;
    context.start_store.remove_readiness_deadline(operation_id);
    context
        .dispatch
        .operation_events
        .extend(readiness.operation_events);
    context
        .dispatch
        .service_transitions
        .extend(readiness.service_transitions);
    context.dispatch.graph_events.extend(readiness.graph_events);
    context.dispatch.post_start_hook = readiness.post_start_hook;
    Ok(())
}
