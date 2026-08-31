use crate::service::{is_valid_service_name, split_target};

use crate::registry::fields::Field;
use crate::registry::value::{
    RawRegistryValue, ServiceRegistryDecodeError, decode_multi_sz_field, decode_sz_field,
};

pub(in crate::registry::service) fn validate_service_name(value: &str) -> Result<(), ()> {
    if is_valid_service_name(value) {
        Ok(())
    } else {
        Err(())
    }
}

pub(in crate::registry::service) fn parse_service_reference_list(
    value: &RawRegistryValue,
    field: Field,
) -> Result<Vec<String>, ServiceRegistryDecodeError> {
    decode_multi_sz_field(value, field.name())?
        .into_iter()
        .map(|entry| {
            validate_dependency_reference(field, &entry)?;
            Ok(entry)
        })
        .collect()
}

/// Validate a `Requires`/`Wants`/`BindsTo`/`Conflicts` entry, which may
/// carry a readiness level: `netd:routed`.
///
/// `Conflicts` cannot usefully name a level — conflicting with a service
/// at one level and not another is not a thing peinit can act on — but it
/// is validated the same way rather than specially, because a rule that
/// applies to three of four list fields and not the fourth is a rule
/// somebody will get wrong. A level on a `Conflicts` entry is inert.
fn validate_dependency_reference(
    field: Field,
    value: &str,
) -> Result<(), ServiceRegistryDecodeError> {
    let (service, level) = split_target(value);
    let valid = is_valid_service_name(&service)
        && level.as_deref().is_none_or(is_valid_level);
    if valid {
        Ok(())
    } else {
        Err(ServiceRegistryDecodeError::InvalidServiceReference {
            field: field.name(),
            value: value.to_string(),
        })
    }
}

/// Is this a level a service could plausibly publish?
///
/// Deliberately permissive: a level is the publisher's own vocabulary and
/// peinit has no business constraining it beyond what it must be able to
/// compare and print. Non-empty, bounded, and printable with no spaces —
/// enough to keep a stray newline or a megabyte of registry data out of a
/// comparison, and no more.
fn is_valid_level(level: &str) -> bool {
    !level.is_empty()
        && level.len() <= 64
        && level.chars().all(|c| c.is_ascii_graphic())
}

pub(in crate::registry::service) fn parse_service_reference_field(
    value: &RawRegistryValue,
    field: Field,
) -> Result<String, ServiceRegistryDecodeError> {
    let parsed = decode_sz_field(value, field.name())?;
    validate_service_reference(field, &parsed)?;
    Ok(parsed)
}

fn validate_service_reference(field: Field, value: &str) -> Result<(), ServiceRegistryDecodeError> {
    validate_service_name(value).map_err(|_| ServiceRegistryDecodeError::InvalidServiceReference {
        field: field.name(),
        value: value.to_string(),
    })
}

#[cfg(test)]
mod level_reference_tests {
    use super::*;

    #[test]
    fn a_level_is_accepted_on_a_dependency() {
        // The bug this test exists for: every Requires entry used to be
        // validated as a service name, so `netd:routed` was rejected at
        // registry parse time and took the whole reload down with it. The
        // consumer side was built and could never be reached.
        assert!(validate_dependency_reference(Field::Requires, "netd:routed").is_ok());
        assert!(validate_dependency_reference(Field::Wants, "timed:synchronised").is_ok());
        assert!(validate_dependency_reference(Field::Requires, "netd").is_ok());
    }

    #[test]
    fn a_bad_service_name_is_still_rejected() {
        assert!(validate_dependency_reference(Field::Requires, "has space").is_err());
        assert!(validate_dependency_reference(Field::Requires, "").is_err());
        assert!(validate_dependency_reference(Field::Requires, ":routed").is_err());
    }

    #[test]
    fn an_implausible_level_is_rejected() {
        assert!(validate_dependency_reference(Field::Requires, "netd:with space").is_err());
        assert!(validate_dependency_reference(Field::Requires, "netd:with\nnewline").is_err());
        let long = format!("netd:{}", "x".repeat(65));
        assert!(validate_dependency_reference(Field::Requires, &long).is_err());
    }

    #[test]
    fn a_trailing_colon_is_the_plain_service() {
        // split_target reads it as a typo rather than an empty level, so
        // it validates as the bare service name it looks like.
        assert!(validate_dependency_reference(Field::Requires, "netd:").is_ok());
    }
}
