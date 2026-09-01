//! Roles: dependencies peinit derives rather than reads.
//!
//! A service declaring `Identity = LocalService` cannot start until the
//! authority is *listening*, because peinit has to ask it for a token first
//! (§4.3). That is a hard dependency in every sense the dependency graph
//! means one — and until this module existed it was written down nowhere.
//! Three of three `LocalService` definitions in the tree omitted it, which is
//! the number that makes this a defect in the mechanism rather than in three
//! definitions: the requirement is created by `Identity`, so `Identity` is
//! what should produce the edge.
//!
//! # Why a role and not a service name
//!
//! peinit hardcodes the socket the authority listens on. It does not know the
//! authority's *name*, which is registry data, and should not: an image is
//! free to ship a different authority. So the derived edge names a **role** —
//! a virtual name some service declares it fills — and peinit resolves it to
//! whatever service says `Provides = ["authn"]`.
//!
//! Virtual and real names share one namespace, exactly as they do for
//! packages (PSPU §5.4). A dependency on `authn` is satisfied by a service
//! literally called `authn`, or by any service providing it.
//!
//! # This is not an `online` target
//!
//! peinit rejects `network-online.target` and says why (§7.5): "online" has no
//! single definition, so a dependency on it cannot mean one thing. A role is
//! not that. `authn` is not a condition several services contribute to; it is
//! "whoever answers `ServiceAttest` on the logon socket", a protocol role that
//! PGSS Logon §2.1 says MUST have at most one occupant. The indirection is
//! over *which service*, never over *what condition* — which is why the
//! objection to targets does not reach it.

use std::collections::BTreeSet;

use crate::security::{hook_execution_identity, is_system_identity};

use super::ServiceDefinition;

/// The role of the authority that mints non-SYSTEM tokens.
///
/// Named `authn` rather than `logon` or `authority` on purpose. Because
/// virtual names share a namespace with real service names, a role wants a
/// spelling nobody would reach for as a *daemon* name — and `logon` is an
/// entirely plausible name for a getty (`login-console` already exists). The
/// value of `authn` here is precisely that it is not a word anyone names a
/// process.
pub const AUTHN_ROLE: &str = "authn";

/// Does starting this service require the authority?
///
/// The predicate mirrors the launcher's, which routes any identity that is
/// not `SYSTEM` to authd and mints the rest itself. Hooks are included
/// because they materialise a token of their own: a `SYSTEM` service whose
/// `HookIdentity` is not SYSTEM still cannot run its hooks without the
/// authority, and a definition that needs authd for half its launch needs the
/// ordering for all of it.
pub(crate) fn requires_authority(definition: &ServiceDefinition) -> bool {
    if !is_system_identity(&definition.identity) {
        return true;
    }
    let has_hooks = !definition.exec_start_pre.is_empty() || !definition.exec_start_post.is_empty();
    has_hooks && !is_system_identity(&hook_execution_identity(definition))
}

/// Add the dependencies implied by each definition's identity.
///
/// Applied to a whole set rather than one definition at a time, because
/// resolving a role needs to know who fills it. Idempotent: a definition that
/// already carries the edge — declared by hand, or synthesised by an earlier
/// pass over the same set — gains nothing.
///
/// A provider never gains a dependency on its own role. Without that, an
/// authority declaring a non-SYSTEM identity would require itself, and a
/// service that requires itself never starts.
pub fn synthesise_role_dependencies(
    mut definitions: Vec<ServiceDefinition>,
) -> Vec<ServiceDefinition> {
    let providers = role_providers(&definitions, AUTHN_ROLE);
    if providers.is_empty() {
        // Nothing fills the role. The edge is deliberately *not* invented
        // against a name no service answers to: peinit would be turning a
        // launch that fails one service into a missing hard dependency, and
        // at reload that rejects the whole transaction. The image is broken
        // either way; this way it breaks where it is true, at the launch that
        // cannot get a token.
        return definitions;
    }

    for definition in &mut definitions {
        if !requires_authority(definition) || definition.provides.iter().any(|r| r == AUTHN_ROLE) {
            continue;
        }
        let declared: BTreeSet<String> = definition
            .requires
            .iter()
            .map(|entry| super::split_target(entry).0)
            .collect();
        let additions: Vec<String> = providers
            .iter()
            .filter(|provider| *provider != &definition.name)
            .filter(|provider| !declared.contains(*provider))
            .cloned()
            .collect();
        definition.requires.extend(additions);
    }
    definitions
}

/// Every service filling `role`, by name.
///
/// More than one is not an error. For `authn` it cannot happen — PGSS Logon
/// §2.1 requires at most one authority on a running system — and where a
/// future role does allow several, ordering after all of them is the
/// conservative reading and needs no new kind of edge: each provider becomes
/// an ordinary `Requires`, with the failure and start semantics those already
/// have.
pub(crate) fn role_providers(definitions: &[ServiceDefinition], role: &str) -> Vec<String> {
    definitions
        .iter()
        .filter(|definition| definition.provides.iter().any(|entry| entry == role))
        .map(|definition| definition.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::LOCAL_SERVICE_IDENTITY as LOCAL_SERVICE;

    fn service(name: &str) -> ServiceDefinition {
        ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"))
    }

    fn authority() -> ServiceDefinition {
        let mut authd = service("authd");
        authd.provides = vec![AUTHN_ROLE.to_string()];
        authd
    }

    fn local_service(name: &str) -> ServiceDefinition {
        let mut definition = service(name);
        definition.identity = LOCAL_SERVICE.to_string();
        definition
    }

    fn requires_of(definitions: &[ServiceDefinition], name: &str) -> Vec<String> {
        definitions
            .iter()
            .find(|definition| definition.name == name)
            .expect("service")
            .requires
            .clone()
    }

    /// PEI-600. This is the whole bug: resolvd declares `LocalService` and
    /// nothing else, and had no ordering against the authority at all.
    #[test]
    fn a_non_system_identity_gains_the_authority_edge() {
        let synthesised = synthesise_role_dependencies(vec![authority(), local_service("resolvd")]);

        assert_eq!(
            requires_of(&synthesised, "resolvd"),
            vec!["authd".to_string()]
        );
    }

    #[test]
    fn a_system_service_gains_nothing() {
        let synthesised = synthesise_role_dependencies(vec![authority(), service("netd")]);

        assert!(requires_of(&synthesised, "netd").is_empty());
    }

    /// A SYSTEM service whose hooks run as something else still cannot launch
    /// without the authority — the hook materialises a token of its own.
    #[test]
    fn a_system_service_with_non_system_hooks_gains_the_edge() {
        let mut hooked = service("hooked");
        hooked.exec_start_pre = vec!["/usr/libexec/prepare".to_string()];
        hooked.hook_identity = Some(LOCAL_SERVICE.to_string());

        let synthesised = synthesise_role_dependencies(vec![authority(), hooked]);

        assert_eq!(
            requires_of(&synthesised, "hooked"),
            vec!["authd".to_string()]
        );
    }

    /// Declaring a hook identity without any hooks asks for nothing, so it
    /// should not order anything either.
    #[test]
    fn a_hook_identity_with_no_hooks_gains_nothing() {
        let mut idle = service("idle");
        idle.hook_identity = Some(LOCAL_SERVICE.to_string());

        let synthesised = synthesise_role_dependencies(vec![authority(), idle]);

        assert!(requires_of(&synthesised, "idle").is_empty());
    }

    /// An authority that declared a non-SYSTEM identity would otherwise
    /// require itself, and a service that requires itself never starts.
    #[test]
    fn the_provider_never_depends_on_its_own_role() {
        let mut authd = authority();
        authd.identity = LOCAL_SERVICE.to_string();

        let synthesised = synthesise_role_dependencies(vec![authd]);

        assert!(requires_of(&synthesised, "authd").is_empty());
    }

    #[test]
    fn a_hand_declared_edge_is_not_duplicated() {
        let mut resolvd = local_service("resolvd");
        resolvd.requires = vec!["authd".to_string()];

        let synthesised = synthesise_role_dependencies(vec![authority(), resolvd]);

        assert_eq!(
            requires_of(&synthesised, "resolvd"),
            vec!["authd".to_string()]
        );
    }

    /// A declared edge carrying a readiness level already orders against the
    /// authority, and more strictly. Adding a plain one beside it would be a
    /// second edge to the same service saying less.
    #[test]
    fn a_declared_level_edge_is_not_duplicated() {
        let mut resolvd = local_service("resolvd");
        resolvd.requires = vec!["authd:ready".to_string()];

        let synthesised = synthesise_role_dependencies(vec![authority(), resolvd]);

        assert_eq!(
            requires_of(&synthesised, "resolvd"),
            vec!["authd:ready".to_string()]
        );
    }

    /// Reload runs this over a set that already went through it once.
    #[test]
    fn synthesis_is_idempotent() {
        let once = synthesise_role_dependencies(vec![authority(), local_service("resolvd")]);
        let twice = synthesise_role_dependencies(once.clone());

        assert_eq!(twice, once);
    }

    /// The deliberate non-behaviour. Inventing an edge to a name no service
    /// answers to would make every reload of this image fail validation —
    /// including the reload that installs the authority.
    #[test]
    fn nothing_is_invented_when_no_service_fills_the_role() {
        let synthesised = synthesise_role_dependencies(vec![local_service("resolvd")]);

        assert!(requires_of(&synthesised, "resolvd").is_empty());
    }

    #[test]
    fn every_provider_is_ordered_against() {
        let mut second = service("otherauthd");
        second.provides = vec![AUTHN_ROLE.to_string()];

        let synthesised =
            synthesise_role_dependencies(vec![authority(), second, local_service("resolvd")]);

        assert_eq!(
            requires_of(&synthesised, "resolvd"),
            vec!["authd".to_string(), "otherauthd".to_string()]
        );
    }
}
