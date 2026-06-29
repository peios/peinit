use super::{OperationRecord, OperationState};

pub const OPERATION_TIMEOUT_RESULT_CODE: &str = "operation_timeout";

pub fn operation_timeout_result(detail: impl AsRef<str>) -> String {
    format!("{OPERATION_TIMEOUT_RESULT_CODE}: {}", detail.as_ref())
}

pub fn is_operation_timeout(operation: &OperationRecord) -> bool {
    operation.state == OperationState::Failed
        && operation
            .result
            .as_deref()
            .is_some_and(is_operation_timeout_result)
}

pub fn is_operation_timeout_result(result: &str) -> bool {
    result == OPERATION_TIMEOUT_RESULT_CODE
        || result.starts_with(&format!("{OPERATION_TIMEOUT_RESULT_CODE}:"))
}
