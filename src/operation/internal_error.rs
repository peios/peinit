use super::{OperationRecord, OperationState};

/// The result code of an operation that failed because peinit could not
/// execute it, as opposed to one whose service failed.
///
/// An operation admitted by the matrix and then refused by execution — a
/// `MissingCurrentMainJob`, an `InvalidTransition` — is a peinit fault, not a
/// service fault. Until PEI-803 it escaped the work pump as a fatal runtime
/// loop error and took PID 1 to recovery; now it fails the one operation, and
/// this code is how a waiting client is told the difference.
pub const INTERNAL_ERROR_RESULT_CODE: &str = "internal_error";

pub fn internal_error_result(detail: impl AsRef<str>) -> String {
    format!("{INTERNAL_ERROR_RESULT_CODE}: {}", detail.as_ref())
}

pub fn is_internal_error(operation: &OperationRecord) -> bool {
    operation.state == OperationState::Failed
        && operation
            .result
            .as_deref()
            .is_some_and(is_internal_error_result)
}

pub fn is_internal_error_result(result: &str) -> bool {
    result == INTERNAL_ERROR_RESULT_CODE
        || result.starts_with(&format!("{INTERNAL_ERROR_RESULT_CODE}:"))
}
