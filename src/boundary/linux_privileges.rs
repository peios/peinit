//! Verifying peinit holds the privileges it is going to need.
//!
//! §13.1 names three privileges peinit requires. Two of them it genuinely
//! needs, and nothing checked for either:
//!
//! - **SeTcbPrivilege**, to request tokens from authd on a service's behalf
//!   and to install a primary token on a child whose identity differs from
//!   peinit's own. Exercised on every service start.
//! - **SeCreateTokenPrivilege**, to mint SYSTEM tokens for platform services
//!   during bootstrap.
//!
//! A third is checked too, and enabled if it is held disabled:
//! **SeAuditPrivilege**, without which peinit's events and the KACS records
//! of the access checks it runs are not written (see [`REQUIRED`]).
//!
//! Without a check, a peinit lacking `SeCreateTokenPrivilege` surfaced it as an
//! `EPERM` from `kacs_create_token` at the *first service start* — which is
//! registryd, in Phase 1 step 6. So the machine entered recovery reporting a
//! token-materialisation failure that read as a registryd problem, and nothing
//! anywhere named the missing privilege. PID 1 discovering it cannot mint
//! tokens is worth saying before it tries (PEI-365).
//!
//! **SeImpersonatePrivilege is not checked, because peinit does not need it.**
//! §13.1 justifies it as needed "to impersonate control socket callers for
//! AccessCheck evaluation", and peinit does not impersonate: it passes the
//! peer's token descriptor to `AccessCheck` directly. There is no impersonation
//! anywhere in the tree. That is worth being precise about rather than
//! requesting the privilege defensively, because the reason peinit does not
//! need it is a good one — evaluating a check against a token you hold a
//! descriptor to is strictly better than assuming the caller's identity, which
//! has a window in which PID 1 is running as somebody else.

use peios::security::Privileges;
use peios::token::{PrivilegeAdjustment, Token, TokenAccess};

use super::BoundaryError;

/// The privileges peinit cannot do its job without.
///
/// **SeAuditPrivilege** writes peinit's events: KMES refuses an emit from a
/// token without it enabled. It also lets an AccessCheck peinit runs produce
/// the `kacs.audit.access.checked` record a descriptor's SACL asks for, which
/// is how a refused control or job command is recorded (PEI-1279). Without it
/// the check is still made but nothing is written, so the machine would run
/// with no record of who was refused what.
const REQUIRED: &[(&str, Privileges)] = &[
    ("SeTcbPrivilege", Privileges::TCB),
    ("SeCreateTokenPrivilege", Privileges::CREATE_TOKEN),
    ("SeAuditPrivilege", Privileges::AUDIT),
];

/// Privileges peinit enables for itself when its token holds them disabled.
/// The boot SYSTEM token holds every privilege enabled, so this changes
/// nothing on a token the kernel made; it keeps a token made some other way
/// from silently writing no audit record.
const ENABLED_AT_START: &[Privileges] = &[Privileges::AUDIT];

/// Check peinit's own token, naming what is missing.
///
/// Present *and* enabled: a privilege the token carries but has not enabled is
/// not usable, and the failure would look identical to it being absent. The
/// privileges in [`ENABLED_AT_START`] are enabled first if present.
pub fn verify_peinit_privileges() -> Result<(), BoundaryError> {
    let token = Token::open_self(true, TokenAccess::QUERY | TokenAccess::ADJUST_PRIVS)
        .map_err(|error| BoundaryError::Token(format!("open peinit token failed: {error}")))?;
    let query = |token: &Token| {
        token.privileges().map_err(|error| {
            BoundaryError::Token(format!("query peinit privileges failed: {error}"))
        })
    };
    let mut privileges = query(&token)?;
    let enable = privileges_to_enable(privileges.present, privileges.enabled);
    if !enable.is_empty() {
        token.adjust_privileges(&enable).map_err(|error| {
            BoundaryError::Token(format!("enable peinit privileges failed: {error}"))
        })?;
        privileges = query(&token)?;
    }
    let missing = missing_privileges(privileges.present, privileges.enabled);
    if missing.is_empty() {
        return Ok(());
    }
    Err(BoundaryError::Token(format!(
        "peinit is missing required privilege(s): {}",
        missing.join(", ")
    )))
}

/// The adjustments that enable each start-time privilege the token holds
/// but has not enabled. A privilege's LUID is its bit's index.
fn privileges_to_enable(present: Privileges, enabled: Privileges) -> Vec<PrivilegeAdjustment> {
    ENABLED_AT_START
        .iter()
        .filter(|bit| present.contains(**bit) && !enabled.contains(**bit))
        .map(|bit| PrivilegeAdjustment::enable(bit.bits().trailing_zeros()))
        .collect()
}

fn missing_privileges(present: Privileges, enabled: Privileges) -> Vec<&'static str> {
    REQUIRED
        .iter()
        .filter(|(_, bit)| !present.contains(*bit) || !enabled.contains(*bit))
        .map(|(name, _)| *name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Privileges = Privileges::TCB
        .union(Privileges::CREATE_TOKEN)
        .union(Privileges::AUDIT);

    #[test]
    fn a_token_holding_all_three_enabled_is_complete() {
        assert!(missing_privileges(ALL, ALL).is_empty());
    }

    // The check is what turns "registryd would not start" into "peinit cannot
    // mint tokens". Naming the specific privilege is the whole value.
    #[test]
    fn each_missing_privilege_is_named() {
        let without = |bit: Privileges| ALL.difference(bit);
        assert_eq!(
            missing_privileges(without(Privileges::CREATE_TOKEN), without(Privileges::CREATE_TOKEN)),
            vec!["SeCreateTokenPrivilege"],
        );
        assert_eq!(
            missing_privileges(without(Privileges::TCB), without(Privileges::TCB)),
            vec!["SeTcbPrivilege"],
        );
        assert_eq!(
            missing_privileges(without(Privileges::AUDIT), without(Privileges::AUDIT)),
            vec!["SeAuditPrivilege"],
        );
        assert_eq!(
            missing_privileges(Privileges::empty(), Privileges::empty()),
            vec!["SeTcbPrivilege", "SeCreateTokenPrivilege", "SeAuditPrivilege"],
        );
    }

    /// Present but not enabled is not usable, and would otherwise fail at the
    /// same place and look identical to being absent.
    #[test]
    fn a_present_but_disabled_privilege_counts_as_missing() {
        assert_eq!(
            missing_privileges(ALL, ALL.difference(Privileges::CREATE_TOKEN)),
            vec!["SeCreateTokenPrivilege"],
        );
    }

    /// SeAuditPrivilege held but disabled is enabled at start, rather than
    /// leaving peinit's events and the access-check records unwritten.
    #[test]
    fn a_disabled_audit_privilege_is_enabled_and_an_absent_one_is_not() {
        let enable = privileges_to_enable(ALL, ALL.difference(Privileges::AUDIT));
        assert_eq!(enable.len(), 1);
        assert_eq!(enable[0].luid, 21, "SeAuditPrivilege is bit 21 of the ABI");

        assert!(privileges_to_enable(ALL, ALL).is_empty());
        assert!(
            privileges_to_enable(ALL.difference(Privileges::AUDIT), Privileges::empty())
                .is_empty(),
            "a privilege the token does not hold cannot be enabled",
        );
    }

    /// SeImpersonatePrivilege is deliberately absent from the list: peinit
    /// passes the caller's token descriptor to AccessCheck rather than
    /// impersonating, so it never needs one.
    #[test]
    fn impersonate_is_not_required() {
        assert!(
            !REQUIRED
                .iter()
                .any(|(name, _)| *name == "SeImpersonatePrivilege"),
        );
    }
}
