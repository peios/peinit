//! PEI-829. The boot plan used to be built from the raw definitions while the
//! service table, reload validation and on-demand starts all saw the
//! role-synthesised set, so the boot graph alone lacked derived edges and
//! resolved role references.

use crate::boot::phase2::{BlockedReason, DependencyKind, StartCause};
use crate::security::LOCAL_SERVICE_IDENTITY;
use crate::service::{AUTHN_ROLE, ServiceDefinition};

use super::plan;

fn service(name: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"))
}

/// A provider with no boot trigger of its own: the only way into the boot
/// closure is through an edge to it.
fn provider(name: &str, role: &str) -> ServiceDefinition {
    let mut definition = service(name);
    definition.triggers.clear();
    definition.provides = vec![role.to_string()];
    definition
}

fn starts(plan: &crate::boot::phase2::Phase2BootPlan) -> Vec<(&str, StartCause)> {
    plan.starts
        .iter()
        .map(|start| (start.service.as_str(), start.cause))
        .collect()
}

/// The edge `Identity = LocalService` derives (§4.3) must pull the authority
/// into the boot closure ahead of the service that needs it, exactly as a
/// declared `Requires` would.
#[test]
fn a_derived_edge_pulls_its_provider_into_the_boot_plan() {
    let mut resolvd = service("resolvd");
    resolvd.identity = LOCAL_SERVICE_IDENTITY.to_string();

    let boot = plan(&[resolvd, provider("authd", AUTHN_ROLE)]);

    assert!(boot.blocked.is_empty(), "{:?}", boot.blocked);
    assert_eq!(
        starts(&boot),
        vec![
            ("authd", StartCause::DependencyStart),
            ("resolvd", StartCause::ExplicitStart),
        ],
    );
}

/// `Requires = ["network:routed"]` names a role with a level. The boot path
/// must resolve it to the provider and carry the level across — not fail
/// the dependent on a missing service called `network`, which is what the
/// raw definitions said.
#[test]
fn a_role_with_a_level_resolves_on_the_boot_path() {
    let mut web = service("web");
    web.requires = vec!["network:routed".to_string()];

    let boot = plan(&[web, provider("netd", "network")]);

    assert!(boot.blocked.is_empty(), "{:?}", boot.blocked);
    assert_eq!(
        starts(&boot),
        vec![
            ("netd", StartCause::DependencyStart),
            ("web", StartCause::ExplicitStart),
        ],
    );
}

/// Nothing is invented for an unfilled role: the dependent is blocked on
/// the role the operator wrote, the same finding validation reports.
#[test]
fn an_unfilled_role_still_blocks_the_dependent_by_the_role_name() {
    let mut web = service("web");
    web.requires = vec!["network:routed".to_string()];

    let boot = plan(&[web]);

    assert!(boot.starts.is_empty());
    assert_eq!(boot.blocked.len(), 1);
    assert_eq!(boot.blocked[0].service, "web");
    assert_eq!(
        boot.blocked[0].reason,
        BlockedReason::HardDependencyUnavailable {
            target: "network".to_string(),
            kind: DependencyKind::Requires,
        },
    );
}
