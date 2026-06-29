use std::str::FromStr;

use peios::security::{GroupAttributes, IntegrityLevel, Sid, WellKnown};
use peios::token::{
    ImpersonationLevel, PrivilegeSet, SessionId, Token, TokenAccess, TokenBuilder, TokenType,
};

use crate::boundary::BoundaryError;
use crate::security::service_sid;

const PEINIT_TOKEN_SOURCE: &str = "peinit";

pub(super) fn create_system_token(service: &str) -> Result<Token, BoundaryError> {
    let template = SystemTokenTemplate::from_self_token()?;
    create_system_token_from_template(service, &template)
}

#[derive(Debug)]
struct SystemTokenTemplate {
    user: Sid,
    groups: Vec<(Sid, u32)>,
    privileges: PrivilegeSet,
    integrity: IntegrityLevel,
    session: SessionId,
}

impl SystemTokenTemplate {
    fn from_self_token() -> Result<Self, BoundaryError> {
        let token = Token::open_self(true, TokenAccess::QUERY).map_err(|error| {
            BoundaryError::Token(format!("open peinit SYSTEM token failed: {error}"))
        })?;
        let user = token.user().map_err(|error| {
            BoundaryError::Token(format!("query SYSTEM user SID failed: {error}"))
        })?;
        if user != Sid::well_known(WellKnown::System) {
            return Err(BoundaryError::Token(format!(
                "peinit real token user is {user:?}, expected SYSTEM"
            )));
        }
        // The kernel re-appends the session's logon SID to every token it
        // creates and rejects (EINVAL) a create whose group list already
        // contains it. Our self token — built by the kernel — carries that
        // logon-SID group, so strip it here; the rebuilt token is given a fresh
        // logon SID for its session.
        let groups = token
            .groups()
            .map_err(|error| BoundaryError::Token(format!("query SYSTEM groups failed: {error}")))?
            .into_iter()
            .filter(|(_, attrs)| {
                !GroupAttributes::from_bits_truncate(*attrs).contains(GroupAttributes::LOGON_ID)
            })
            .collect();
        let privileges = token.privileges().map_err(|error| {
            BoundaryError::Token(format!("query SYSTEM privileges failed: {error}"))
        })?;
        let integrity = token.integrity().map_err(|error| {
            BoundaryError::Token(format!("query SYSTEM integrity failed: {error}"))
        })?;
        let session = token.session_id().map_err(|error| {
            BoundaryError::Token(format!("query SYSTEM session failed: {error}"))
        })?;
        Ok(Self {
            user,
            groups,
            privileges,
            integrity,
            session,
        })
    }
}

fn create_system_token_from_template(
    service: &str,
    template: &SystemTokenTemplate,
) -> Result<Token, BoundaryError> {
    let service = Sid::from_str(&service_sid(service))
        .map_err(|error| BoundaryError::Token(format!("derive service SID failed: {error}")))?;
    let group_attrs = GroupAttributes::MANDATORY
        .union(GroupAttributes::ENABLED_BY_DEFAULT)
        .union(GroupAttributes::ENABLED);
    let mut builder = TokenBuilder::new();
    builder
        .user(&template.user)
        // A primary token's impersonation level must be Anonymous — the level
        // only has meaning for impersonation tokens, and the kernel rejects a
        // Primary token carrying any other level with EINVAL.
        .token_type(TokenType::Primary, ImpersonationLevel::Anonymous)
        .integrity(template.integrity)
        .privileges(template.privileges.present, template.privileges.enabled)
        .session(template.session)
        .owner_index(0)
        .primary_group_index(0)
        .projected_ids(0, 0)
        .source(PEINIT_TOKEN_SOURCE, 0);
    for (sid, attrs) in &template.groups {
        builder.add_group(sid, *attrs);
    }
    builder.add_group(&service, group_attrs.bits());
    builder
        .create()
        .map_err(|error| BoundaryError::Token(format!("create SYSTEM token failed: {error}")))
}
