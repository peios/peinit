use crate::job::JobRecord;
use crate::security::TokenSummary;

use crate::boundary::BoundaryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenHandle {
    pub fd: i32,
    pub identity: String,
    pub summary: TokenSummary,
}

pub trait TokenProvider {
    fn materialize_service_token(&mut self, job: &JobRecord) -> Result<TokenHandle, BoundaryError>;

    /// Wrap a primary token the submission path already prepared (a
    /// `PreparedJobIdentity`) as the handle the launch installs. The core
    /// never sees the token; it hands the descriptor back to the boundary
    /// that made it.
    fn materialize_prepared_token(
        &mut self,
        job: &JobRecord,
        prepared_token_fd: i32,
    ) -> Result<TokenHandle, BoundaryError> {
        let _ = (job, prepared_token_fd);
        Err(BoundaryError::Token(
            "prepared token materialisation unavailable".to_string(),
        ))
    }
}
