use crate::service::ServiceDefinition;

pub const SYSTEM_IDENTITY: &str = "SYSTEM";
pub const LOCAL_SERVICE_IDENTITY: &str = "LocalService";
pub const NETWORK_SERVICE_IDENTITY: &str = "NetworkService";
pub const DEFAULT_SERVICE_IDENTITY: &str = LOCAL_SERVICE_IDENTITY;

pub fn is_system_identity(identity: &str) -> bool {
    identity == SYSTEM_IDENTITY
}

pub fn canonical_well_known_identity(identity: &str) -> Option<&'static str> {
    if identity.eq_ignore_ascii_case(SYSTEM_IDENTITY) {
        Some(SYSTEM_IDENTITY)
    } else if identity.eq_ignore_ascii_case(LOCAL_SERVICE_IDENTITY) {
        Some(LOCAL_SERVICE_IDENTITY)
    } else if identity.eq_ignore_ascii_case(NETWORK_SERVICE_IDENTITY) {
        Some(NETWORK_SERVICE_IDENTITY)
    } else {
        None
    }
}

pub fn hook_execution_identity(definition: &ServiceDefinition) -> String {
    definition
        .hook_identity
        .clone()
        .unwrap_or_else(|| definition.identity.clone())
}

pub fn identity_user_sid(identity: &str) -> Option<String> {
    match identity {
        SYSTEM_IDENTITY => Some("S-1-5-18".to_string()),
        LOCAL_SERVICE_IDENTITY => Some("S-1-5-19".to_string()),
        NETWORK_SERVICE_IDENTITY => Some("S-1-5-20".to_string()),
        literal if is_literal_sid(literal) => Some(literal.to_string()),
        _ => None,
    }
}

pub fn is_literal_sid(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("S-") else {
        return false;
    };
    let mut parts = rest.split('-');
    let Some(revision) = parts.next() else {
        return false;
    };
    let Some(authority) = parts.next() else {
        return false;
    };
    !revision.is_empty()
        && !authority.is_empty()
        && revision.bytes().all(|byte| byte.is_ascii_digit())
        && authority.bytes().all(|byte| byte.is_ascii_digit())
        && parts.clone().next().is_some()
        && parts.all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}
