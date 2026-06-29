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
}
