use crate::boundary::ProcessController;
use crate::notify::NotifyMessage;

use super::auth::authenticate_notify_sender;
use super::field::{NotifyFieldContext, apply_notify_field};
use super::model::{NotifyApplyContext, NotifyApplyDispatch, NotifyApplyError, NotifyApplyRequest};

pub fn apply_notify_message<P>(
    context: NotifyApplyContext<'_, P>,
    request: NotifyApplyRequest,
    message: &NotifyMessage,
) -> Result<NotifyApplyDispatch, NotifyApplyError>
where
    P: ProcessController + ?Sized,
{
    let sender = authenticate_notify_sender(
        context.services,
        &*context.jobs,
        context.controller,
        request.sender_pid,
    )?;
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_graph = context.graph.clone();
    let mut next_jobs = context.jobs.clone();
    let mut next_job_ids = context.job_ids.clone();
    let mut next_start_store = context.start_store.clone();
    let mut next_control_store = context.control_store.clone();
    let mut dispatch = NotifyApplyDispatch {
        sender: sender.clone(),
        applied_fields: Vec::new(),
        operation_events: Vec::new(),
        service_transitions: Vec::new(),
        graph_events: Vec::new(),
        post_start_hook: None,
    };

    for field in &message.fields {
        let mut field_context = NotifyFieldContext {
            services: &mut next_services,
            operations: &mut next_operations,
            graph: &mut next_graph,
            jobs: &mut next_jobs,
            job_ids: &mut next_job_ids,
            start_store: &mut next_start_store,
            control_store: &mut next_control_store,
            sender: &sender,
            observed_at_ns: request.observed_at_ns,
            dispatch: &mut dispatch,
        };
        apply_notify_field(&mut field_context, field)?;
    }

    *context.services = next_services;
    *context.operations = next_operations;
    *context.graph = next_graph;
    *context.jobs = next_jobs;
    *context.job_ids = next_job_ids;
    *context.start_store = next_start_store;
    *context.control_store = next_control_store;

    Ok(dispatch)
}
