use std::str::FromStr;

use peios::security::{GroupAttributes, IntegrityLevel, Sid, WellKnown, sddl};
use peios::token::{
    ImpersonationLevel, PrivilegeSet, SessionId, Token, TokenAccess, TokenBuilder, TokenType,
};

use crate::boundary::BoundaryError;
use crate::security::service_sid;

const PEINIT_TOKEN_SOURCE: &str = "peinit";

/// The DACL an object created by a SYSTEM service gets when it has no parent
/// to inherit from: SYSTEM and Administrators full control, nobody else.
///
/// The same DACL the bootstrap SYSTEM token carries, and stated here rather
/// than copied from the template because the template's default DACL is not
/// something `Token` can query. The kernel supplies no fallback of its own --
/// a token with an empty default DACL leaves such an object with a *null*
/// DACL, which grants everything to everybody -- so a platform daemon that
/// binds an abstract socket or creates a key under a container written
/// without inheritable ACEs would otherwise publish it world-writable.
/// Objects with a parent never reach this: every root descriptor peinit
/// stamps carries inheritable ACEs.
const SYSTEM_DEFAULT_DACL_SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;BA)";

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
    auth_id: SessionId,
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
        let statistics = token.statistics().map_err(|error| {
            BoundaryError::Token(format!("query SYSTEM token statistics failed: {error}"))
        })?;
        if statistics.token_type != TokenType::Primary {
            return Err(BoundaryError::Token(format!(
                "peinit real token is {:?}, expected Primary",
                statistics.token_type
            )));
        }
        Ok(Self {
            user,
            groups,
            privileges,
            integrity,
            auth_id: statistics.auth_id,
        })
    }
}

fn create_system_token_from_template(
    service: &str,
    template: &SystemTokenTemplate,
) -> Result<Token, BoundaryError> {
    build_system_token(service, template)?
        .create()
        .map_err(|error| BoundaryError::Token(format!("create SYSTEM token failed: {error}")))
}

fn build_system_token(
    service: &str,
    template: &SystemTokenTemplate,
) -> Result<TokenBuilder, BoundaryError> {
    let service = Sid::from_str(&service_sid(service))
        .map_err(|error| BoundaryError::Token(format!("derive service SID failed: {error}")))?;
    let group_attrs = GroupAttributes::MANDATORY
        .union(GroupAttributes::ENABLED_BY_DEFAULT)
        .union(GroupAttributes::ENABLED);
    let default_dacl = sddl::parse_acl(SYSTEM_DEFAULT_DACL_SDDL).map_err(|error| {
        BoundaryError::Token(format!("parse the SYSTEM default DACL failed: {error}"))
    })?;
    let mut builder = TokenBuilder::new();
    builder
        .user(&template.user)
        .default_dacl(&default_dacl)
        // The impersonation level is a ratchet on every token: nothing captured
        // from, conveyed by, or duplicated out of this token can act above it.
        // A SYSTEM service starts at the top, like the bootstrap SYSTEM token
        // it is copied from. (Kernel TRM §3.5.1)
        .token_type(TokenType::Primary, ImpersonationLevel::Delegation)
        .integrity(template.integrity)
        .privileges(template.privileges.present, template.privileges.enabled)
        // The create-spec field is the LogonSession LUID/auth_id. It is not
        // the independent u32 interactivity scope.
        .session(template.auth_id)
        .owner_index(0)
        .primary_group_index(0)
        .projected_ids(0, 0)
        .source(PEINIT_TOKEN_SOURCE, 0);
    for (sid, attrs) in &template.groups {
        builder.add_group(sid, *attrs);
    }
    builder.add_group(&service, group_attrs.bits());
    // S-1-5-6, the Service group. Every token the authority mints for a service
    // logon carries it, derived from the logon type; a SYSTEM service minted
    // here is no less a service, and one that did not carry it would differ
    // from its neighbours in a way nothing intends.
    //
    // Load-bearing rather than tidy: it is the grantee on anything reachable by
    // services as a class -- peinit's own notify socket first among them -- so
    // omitting it here means every SYSTEM service silently losing the ability
    // to report itself ready.
    builder.add_group(&service_group()?, group_attrs.bits());
    Ok(builder)
}

/// `S-1-5-6` — the group naming a process running under a service logon.
///
/// Built rather than named: `WellKnown` has no `Service` variant, because the
/// enum mirrors a C ABI constant table and adding one is an ABI change. The
/// authority derives the same SID the same way, from its logon-type table.
fn service_group() -> Result<Sid, BoundaryError> {
    Sid::build(5, &[6])
        .map_err(|error| BoundaryError::Token(format!("build the Service SID failed: {error}")))
}

#[cfg(test)]
mod tests {
    use peios::security::{IntegrityLevel, Privileges, Sid, WellKnown, sddl};
    use peios::token::{PrivilegeSet, SessionId};

    use super::{SYSTEM_DEFAULT_DACL_SDDL, SystemTokenTemplate, build_system_token};

    fn template() -> SystemTokenTemplate {
        SystemTokenTemplate {
            user: Sid::well_known(WellKnown::System),
            groups: Vec::new(),
            privileges: PrivilegeSet {
                present: Privileges::empty(),
                enabled: Privileges::empty(),
                enabled_by_default: Privileges::empty(),
                used: Privileges::empty(),
            },
            integrity: IntegrityLevel::SYSTEM,
            auth_id: SessionId(999),
        }
    }

    /// A SYSTEM service token carries a default DACL, so an object it
    /// creates with no parent to inherit from is administrator-only rather
    /// than left with a null DACL.
    #[test]
    fn system_token_spec_carries_the_system_default_dacl() {
        let spec = build_system_token("registryd", &template())
            .expect("build SYSTEM token")
            .to_bytes()
            .expect("serialize SYSTEM token");

        let dacl = sddl::parse_acl(SYSTEM_DEFAULT_DACL_SDDL).expect("the shipped SDDL parses");
        let dacl = dacl.as_bytes();
        assert!(!dacl.is_empty(), "the default DACL must not be empty");
        assert!(
            spec.windows(dacl.len()).any(|window| window == dacl),
            "the create spec must embed the SYSTEM default DACL"
        );
    }

    #[test]
    fn system_token_spec_uses_auth_id_not_interactivity_scope() {
        let template = template();

        let spec = build_system_token("registryd", &template)
            .expect("build SYSTEM token")
            .to_bytes()
            .expect("serialize SYSTEM token");

        assert_eq!(
            u64::from_le_bytes(spec[56..64].try_into().expect("auth_id field")),
            999
        );
        assert_eq!(
            u32::from_le_bytes(
                spec[184..188]
                    .try_into()
                    .expect("interactivity-scope field")
            ),
            0
        );
    }
}
