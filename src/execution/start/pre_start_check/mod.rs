mod passed;
mod terminal;
mod transaction;

use crate::boundary::{FilesystemCheckReport, FilesystemCheckResult};

use super::checks::{
    PreStartCheckDecision, evaluate_pre_start_checks_with_filesystem_results, format_check,
};
use super::model::{
    PreStartCheckCompletionContext, PreStartCheckCompletionDispatch, PreStartCheckTimeoutDispatch,
    StartExecutionContext, StartExecutionError,
};
use super::store::{PendingPreStartCheck, PreStartCheckDeadline};

use transaction::PreStartCheckTransaction;

pub fn complete_pre_start_check_helper(
    context: &mut PreStartCheckCompletionContext<'_>,
    result_fd: i32,
    report: FilesystemCheckReport,
) -> Result<PreStartCheckCompletionDispatch, StartExecutionError> {
    let mut transaction = PreStartCheckTransaction::from_completion_context(context);

    let running = transaction
        .start_store
        .remove_running_pre_start_check_helper_by_result_fd(result_fd)
        .ok_or(StartExecutionError::UnknownPreStartCheckHelper { result_fd })?;
    if running.pending.operation_id != report.operation_id {
        return Err(StartExecutionError::MismatchedPreStartCheckReport {
            expected_operation_id: running.pending.operation_id,
            actual_operation_id: report.operation_id,
        });
    }

    let dispatch = complete_pre_start_check_from_pending(
        &mut transaction,
        result_fd,
        running.pending,
        &report.results,
    )?;

    transaction.commit_to_completion_context(context);

    Ok(dispatch)
}

pub fn timeout_pre_start_check_helper<P>(
    context: &mut StartExecutionContext<'_, P>,
    deadline: PreStartCheckDeadline,
    now_ns: u64,
) -> Result<PreStartCheckTimeoutDispatch, StartExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    let mut transaction = PreStartCheckTransaction::from_start_context(context);

    transaction
        .start_store
        .remove_pre_start_check_deadline(deadline.operation_id);
    let running = transaction
        .start_store
        .remove_running_pre_start_check_helper_by_operation_id(deadline.operation_id)
        .ok_or(StartExecutionError::UnknownPreStartCheckHelper {
            result_fd: deadline.result_fd,
        })?;
    context
        .controller
        .kill_cgroup(&deadline.helper_cgroup_id)
        .map_err(StartExecutionError::Boundary)?;
    let results = failed_results(&running.pending);
    let completion = complete_pre_start_check_from_pending(
        &mut transaction,
        deadline.result_fd,
        running.pending,
        &results,
    )?;

    transaction.commit_to_start_context(context);

    Ok(PreStartCheckTimeoutDispatch {
        completion,
        pidfd: deadline.pidfd,
        killed_cgroup_id: deadline.helper_cgroup_id,
        timed_out_at_ns: now_ns,
    })
}

fn complete_pre_start_check_from_pending(
    transaction: &mut PreStartCheckTransaction,
    result_fd: i32,
    pending: PendingPreStartCheck,
    results: &[FilesystemCheckResult],
) -> Result<PreStartCheckCompletionDispatch, StartExecutionError> {
    match evaluate_pre_start_checks_with_filesystem_results(
        &transaction.services,
        &pending.service,
        &pending.activation.definition.clone(),
        results,
    ) {
        PreStartCheckDecision::Passed => {
            passed::apply_check_passed(transaction, result_fd, pending)
        }
        PreStartCheckDecision::Skipped(reason) => {
            let (cause, message) = (reason.cause(), reason.message());
            terminal::apply_skipped(transaction, result_fd, pending, cause, message)
        }
        PreStartCheckDecision::AssertionFailed(check) => terminal::apply_assertion_failed(
            transaction,
            result_fd,
            pending,
            format!("AssertionError: {} not satisfied", format_check(&check)),
        ),
        PreStartCheckDecision::RequiresFilesystemHelper { .. } => {
            Err(StartExecutionError::UnexpectedFilesystemCheckContinuation {
                operation_id: pending.operation_id,
            })
        }
    }
}

fn failed_results(pending: &PendingPreStartCheck) -> Vec<FilesystemCheckResult> {
    pending
        .checks
        .iter()
        .cloned()
        .map(|check| FilesystemCheckResult {
            check,
            satisfied: false,
        })
        .collect()
}
