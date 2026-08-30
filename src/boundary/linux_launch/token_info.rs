use peios::security::Privileges;
use peios::token::{PrivilegeAdjustment, Token};

use crate::boundary::BoundaryError;
use crate::security::{TokenSummary, identity_user_sid};

/// Resolve `RequiredPrivileges` names to the mask they describe.
///
/// The name↔bit table lives in libpeios, beside the `Privileges` constants it
/// names, and every bit in it comes from the ABI headers. peinit used to carry
/// its own copy with the bit numbers written out by hand, and four of them were
/// wrong — `SeCreateTokenPrivilege` was recorded as bit 0 where the ABI assigns
/// bit 2, with the same off-by-two for the three after it. Because the caller
/// *removes* every present bit outside this mask, a service naming one of those
/// four had it stripped rather than kept: fail-safe, and silent.
fn required_mask(names: &[String]) -> Result<u64, &str> {
    let mut mask = Privileges::empty();
    for name in names {
        let Some(privilege) = Privileges::parse_name(name) else {
            return Err(name);
        };
        mask |= privilege;
    }
    Ok(mask.bits())
}

pub(super) fn apply_required_privileges(
    token: &Token,
    required_privileges: &[String],
) -> Result<(), BoundaryError> {
    if required_privileges.is_empty() {
        return Ok(());
    }
    let required_mask = required_mask(required_privileges).map_err(|name| {
        BoundaryError::Token(format!("unknown RequiredPrivileges entry '{name}'"))
    })?;
    let present = token
        .privileges()
        .map_err(|error| BoundaryError::Token(format!("query token privileges failed: {error}")))?
        .present
        .bits();
    let entries = privileges_to_remove(present, required_mask);
    if entries.is_empty() {
        return Ok(());
    }
    token.adjust_privileges(&entries).map_err(|error| {
        BoundaryError::Token(format!("adjust token privileges failed: {error}"))
    })?;
    Ok(())
}

/// Describe a token, refusing to describe it as an identity it does not carry.
///
/// `identity` is what the service *declared*; `user_sid` is what the token
/// actually holds. Those two disagreeing used to be the normal case rather than
/// an error: peinit's authd client was a stub that returned a SYSTEM token for
/// every identity, so a service declaring `LocalService` ran as `S-1-5-18` and
/// this function reported `LocalService` beside it. `svctl status` therefore
/// described a fully privileged SYSTEM process as `LocalService`, which is what
/// kept the bug invisible from outside for as long as it lasted.
///
/// With a real authority the two can no longer legitimately differ, so a
/// mismatch is a defect somewhere — and failing the launch is the right answer,
/// because the alternative is running a process as a principal nobody asked
/// for while reporting the one they did.
///
/// Only checked where the declared identity *predicts* a SID.
/// [`identity_user_sid`] answers `None` for a principal name, which the
/// authority resolves and this crate cannot; there is nothing to compare
/// against, and inventing a comparison would refuse every such service.
pub(super) fn summarize_token(
    identity: &str,
    token: &Token,
) -> Result<TokenSummary, BoundaryError> {
    let user_sid = token
        .user()
        .map_err(|error| BoundaryError::Token(format!("query token user SID failed: {error}")))?
        .to_string();

    if let Some(expected) = identity_user_sid(identity)
        && expected != user_sid
    {
        return Err(BoundaryError::Token(format!(
            "the token issued for identity '{identity}' carries {user_sid}, not {expected}"
        )));
    }
    let group_sids = token
        .groups()
        .map_err(|error| BoundaryError::Token(format!("query token groups failed: {error}")))?
        .into_iter()
        .map(|(sid, _attrs)| sid.to_string())
        .collect();
    let privileges = token
        .privileges()
        .map_err(|error| BoundaryError::Token(format!("query token privileges failed: {error}")))?;
    Ok(TokenSummary::new(
        identity,
        user_sid,
        group_sids,
        names(privileges.present),
        names(privileges.enabled),
    ))
}

/// A summary is for a human to read, so an unnameable bit is omitted rather
/// than rendered as a number — `canonical_names` skips what it cannot name.
fn names(privileges: Privileges) -> Vec<String> {
    privileges.canonical_names().map(String::from).collect()
}

/// Every present bit the caller did not ask for.
///
/// Deliberately iterates all 64 bits rather than only the named ones: a token
/// may carry a privilege this build has no name for, and `RequiredPrivileges`
/// means *only these*, so an unnameable bit must still be removed.
fn privileges_to_remove(present_mask: u64, required_mask: u64) -> Vec<PrivilegeAdjustment> {
    (0..u64::BITS)
        .filter(|bit| {
            let mask = 1_u64 << bit;
            present_mask & mask != 0 && required_mask & mask == 0
        })
        .map(PrivilegeAdjustment::remove)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{privileges_to_remove, required_mask};
    use peios::security::Privileges;

    #[test]
    fn required_privileges_remove_only_present_unrequested_bits() {
        let present = (1u64 << 21) | (1u64 << 23) | (1u64 << 29);
        let required = 1u64 << 23;
        let removals = privileges_to_remove(present, required);

        assert_eq!(removals.len(), 2);
        assert_eq!(removals[0].luid, 21);
        assert_eq!(removals[1].luid, 29);
    }

    #[test]
    fn required_privileges_remove_present_bits_outside_named_catalog() {
        let present = (1u64 << 23) | (1u64 << 61);
        let required = 1u64 << 23;
        let removals = privileges_to_remove(present, required);

        assert_eq!(removals.len(), 1);
        assert_eq!(removals[0].luid, 61);
    }

    /// The regression the shared table exists to prevent. peinit's own copy had
    /// these as bits 0..3; the ABI assigns 2..5.
    #[test]
    fn the_low_privileges_resolve_to_the_bits_the_abi_assigns() {
        let names = [
            ("SeCreateTokenPrivilege", 2),
            ("SeAssignPrimaryTokenPrivilege", 3),
            ("SeLockMemoryPrivilege", 4),
            ("SeIncreaseQuotaPrivilege", 5),
        ];
        for (name, bit) in names {
            assert_eq!(
                required_mask(&[name.to_string()]),
                Ok(1u64 << bit),
                "{name} must resolve to bit {bit}"
            );
        }
    }

    #[test]
    fn several_names_combine_into_one_mask() {
        let asked = [
            "SeTcbPrivilege".to_string(),
            "SeBackupPrivilege".to_string(),
        ];
        assert_eq!(
            required_mask(&asked),
            Ok((Privileges::TCB | Privileges::BACKUP).bits())
        );
    }

    #[test]
    fn an_unknown_name_is_reported_rather_than_ignored() {
        let asked = [
            "SeTcbPrivilege".to_string(),
            "SeNonsensePrivilege".to_string(),
        ];
        assert_eq!(required_mask(&asked), Err("SeNonsensePrivilege"));
    }

    #[test]
    fn no_names_is_an_empty_mask() {
        assert_eq!(required_mask(&[]), Ok(0));
    }
}
