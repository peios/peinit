#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSummary {
    pub identity: String,
    pub user_sid: String,
    pub group_sids: Vec<String>,
    pub present_privileges: Vec<String>,
    pub enabled_privileges: Vec<String>,
}

impl TokenSummary {
    pub fn new(
        identity: impl Into<String>,
        user_sid: impl Into<String>,
        group_sids: Vec<String>,
        present_privileges: Vec<String>,
        enabled_privileges: Vec<String>,
    ) -> Self {
        Self {
            identity: identity.into(),
            user_sid: user_sid.into(),
            group_sids,
            present_privileges,
            enabled_privileges,
        }
    }

    pub fn requested_identity(identity: impl Into<String>) -> Self {
        let identity = identity.into();
        Self {
            user_sid: super::identity_user_sid(&identity).unwrap_or_default(),
            identity,
            group_sids: Vec::new(),
            present_privileges: Vec::new(),
            enabled_privileges: Vec::new(),
        }
    }

    pub fn caller_sid(&self) -> &str {
        if self.user_sid.is_empty() {
            &self.identity
        } else {
            &self.user_sid
        }
    }
}
