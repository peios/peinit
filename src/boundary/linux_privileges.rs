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
use peios::token::{Token, TokenAccess};

use super::BoundaryError;

/// The privileges peinit cannot do its job without.
const REQUIRED: &[(&str, Privileges)] = &[
    ("SeTcbPrivilege", Privileges::TCB),
    ("SeCreateTokenPrivilege", Privileges::CREATE_TOKEN),
];

/// Check peinit's own token, naming what is missing.
///
/// Present *and* enabled: a privilege the token carries but has not enabled is
/// not usable, and the failure would look identical to it being absent.
pub fn verify_peinit_privileges() -> Result<(), BoundaryError> {
    let token = Token::open_self(true, TokenAccess::QUERY)
        .map_err(|error| BoundaryError::Token(format!("open peinit token failed: {error}")))?;
    let privileges = token.privileges().map_err(|error| {
        BoundaryError::Token(format!("query peinit privileges failed: {error}"))
    })?;
    let missing = missing_privileges(privileges.present, privileges.enabled);
    if missing.is_empty() {
        return Ok(());
    }
    Err(BoundaryError::Token(format!(
        "peinit is missing required privilege(s): {}",
        missing.join(", ")
    )))
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

    #[test]
    fn a_token_holding_both_enabled_is_complete() {
        let all = Privileges::TCB | Privileges::CREATE_TOKEN;

        assert!(missing_privileges(all, all).is_empty());
    }

    // The check is what turns "registryd would not start" into "peinit cannot
    // mint tokens". Naming the specific privilege is the whole value.
    #[test]
    fn each_missing_privilege_is_named() {
        assert_eq!(
            missing_privileges(Privileges::TCB, Privileges::TCB),
            vec!["SeCreateTokenPrivilege"],
        );
        assert_eq!(
            missing_privileges(Privileges::CREATE_TOKEN, Privileges::CREATE_TOKEN),
            vec!["SeTcbPrivilege"],
        );
        assert_eq!(
            missing_privileges(Privileges::empty(), Privileges::empty()),
            vec!["SeTcbPrivilege", "SeCreateTokenPrivilege"],
        );
    }

    /// Present but not enabled is not usable, and would otherwise fail at the
    /// same place and look identical to being absent.
    #[test]
    fn a_present_but_disabled_privilege_counts_as_missing() {
        let both = Privileges::TCB | Privileges::CREATE_TOKEN;

        assert_eq!(
            missing_privileges(both, Privileges::TCB),
            vec!["SeCreateTokenPrivilege"],
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
