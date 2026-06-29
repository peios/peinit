use peios::token::{PrivilegeAdjustment, Token};

use crate::boundary::BoundaryError;
use crate::security::{TokenSummary, privilege_names_from_mask, privilege_request_mask};

pub(super) fn apply_required_privileges(
    token: &Token,
    required_privileges: &[String],
) -> Result<(), BoundaryError> {
    if required_privileges.is_empty() {
        return Ok(());
    }
    let required_mask = privilege_request_mask(required_privileges).map_err(|error| {
        BoundaryError::Token(format!("unknown RequiredPrivileges entry '{}'", error.name))
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

pub(super) fn summarize_token(
    identity: &str,
    token: &Token,
) -> Result<TokenSummary, BoundaryError> {
    let user_sid = token
        .user()
        .map_err(|error| BoundaryError::Token(format!("query token user SID failed: {error}")))?
        .to_string();
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
        privilege_names_from_mask(privileges.present.bits()),
        privilege_names_from_mask(privileges.enabled.bits()),
    ))
}

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
    use super::privileges_to_remove;

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
}
