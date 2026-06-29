use peios::token::Token;

use crate::boundary::BoundaryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AuthdTokenRequest {
    pub identity: String,
    pub service: String,
}

pub(super) trait AuthdTokenClient {
    fn request_service_token(&mut self, request: AuthdTokenRequest)
    -> Result<Token, BoundaryError>;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct HardcodedAuthdTokenClient;

impl AuthdTokenClient for HardcodedAuthdTokenClient {
    fn request_service_token(
        &mut self,
        request: AuthdTokenRequest,
    ) -> Result<Token, BoundaryError> {
        // Authd integration is intentionally deferred; keep the placeholder at
        // this boundary by returning a separately minted SYSTEM token.
        super::system_token::create_system_token(&request.service)
    }
}
