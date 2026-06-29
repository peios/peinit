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
