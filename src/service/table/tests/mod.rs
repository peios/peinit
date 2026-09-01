mod activation;
mod reload;

use crate::service::runtime::{ServiceTransition, TransitionCause};
use crate::service::{ServiceDefinition, ServiceTable};

fn service(name: &str, image_path: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, image_path)
}

fn table(names: &[&str]) -> ServiceTable {
    ServiceTable::from_boot_snapshot(
        names
            .iter()
            .map(|name| service(name, &format!("/sbin/{name}")))
            .collect(),
    )
    .expect("service table")
}

fn transition(
    to: crate::service::runtime::ServiceState,
    cause: TransitionCause,
) -> ServiceTransition {
    ServiceTransition { to, cause }
}

/// PEI-601. The synthesis lives in `map_definitions`, which is the one point
/// both the boot table and a reload go through. These two tests pin that:
/// an edge present at boot and absent after the first reload would be worse
/// than no edge, because the ordering would hold until something reloaded.
fn authority_and_client() -> Vec<ServiceDefinition> {
    let mut authd = service("authd", "/sbin/authd");
    authd.provides = vec![crate::service::AUTHN_ROLE.to_string()];
    let mut resolvd = service("resolvd", "/sbin/resolvd");
    resolvd.identity = crate::security::LOCAL_SERVICE_IDENTITY.to_string();
    vec![authd, resolvd]
}

#[test]
fn the_boot_table_carries_the_synthesised_authority_edge() {
    let table = ServiceTable::from_boot_snapshot(authority_and_client()).expect("service table");

    assert_eq!(
        table.definition("resolvd").expect("resolvd").requires,
        vec!["authd".to_string()]
    );
}

#[test]
fn a_reload_carries_the_synthesised_authority_edge_too() {
    let mut table = ServiceTable::from_boot_snapshot(vec![service("authd", "/sbin/authd")])
        .expect("service table");

    table
        .apply_definition_snapshot(authority_and_client())
        .expect("reload");

    assert_eq!(
        table.definition("resolvd").expect("resolvd").requires,
        vec!["authd".to_string()]
    );
}
