use crate::operation::OperationType;
use crate::operation::store::OperationStoreError;

use super::model::{
    ControlExecutionContext, ControlExecutionDispatch, ControlExecutionError,
    ControlOperationRequest,
};
use super::reload::begin_reload_operation;
use super::stop::begin_stop_like_operation;
use super::target::process_target;

pub fn begin_control_operation<P>(
    context: &mut ControlExecutionContext<'_, P>,
    request: ControlOperationRequest,
) -> Result<ControlExecutionDispatch, ControlExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    let operation = context
        .operations
        .get(request.operation_id)
        .ok_or(OperationStoreError::UnknownOperation {
            id: request.operation_id,
        })
        .map_err(ControlExecutionError::OperationStore)?
        .clone();
    let target = process_target(context.jobs, &operation.service)?;
    let definition = context
        .services
        .definition(&operation.service)
        .ok_or_else(|| {
            ControlExecutionError::ServiceTable(crate::service::ServiceTableError::UnknownService {
                service: operation.service.clone(),
            })
        })?
        .clone();

    match operation.operation_type {
        OperationType::Stop | OperationType::Restart => begin_stop_like_operation(
            context,
            request,
            operation.operation_type,
            target,
            &definition,
        ),
        OperationType::Reload => begin_reload_operation(context, request, target, &definition),
        operation_type => Err(ControlExecutionError::UnsupportedOperationType {
            operation_id: request.operation_id,
            operation_type,
        }),
    }
}
