//! PEI-621: a definition that will not decode fails that service, not the
//! reload.
//!
//! The boot path has always worked this way (§2.5): the planner blocks the
//! undecodable key with `ValidationError` and starts everything else. Reload
//! refused the whole transaction instead, so one typo in one key meant the
//! operator's reload silently changed nothing — including the unrelated
//! definitions they were actually trying to load.

use crate::control::lifecycle::{
    DependencyAvailability, StartBlockReason, StartPlanBlockedService, plan_on_demand_start,
};
use crate::control::reload_config::reload_config;
use crate::service::ServiceDependencyKind;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use super::{StaticRegistry, service, service_table};

const BROKEN: &str = "broken";
const MESSAGE: &str = "MalformedString { field: \"ImagePath\", reason: MissingTerminator }";

#[test]
fn an_undecodable_key_fails_only_that_service_and_the_rest_of_the_batch_loads() {
    let mut services = service_table(&["app"]);
    let mut registry = StaticRegistry::services(vec![
        service("app", "/sbin/app-v2"),
        service("new", "/sbin/new"),
    ])
    .undecodable(BROKEN, Some("ImagePath"), MESSAGE);

    let outcome = reload_config(&mut registry, &mut services).expect("the reload is not refused");

    // The well-formed definitions in the same batch are applied.
    assert_eq!(outcome.summary.added, vec!["new"]);
    assert_eq!(outcome.summary.updated, vec!["app"]);
    assert_eq!(
        services.definition("app").expect("app").image_path,
        "/sbin/app-v2"
    );
    // The broken one is reported by name in the summary, and in detail.
    assert_eq!(outcome.summary.undecodable, vec![BROKEN]);
    assert_eq!(outcome.undecodable.len(), 1);
    assert_eq!(outcome.undecodable[0].name, BROKEN);
    assert_eq!(outcome.undecodable[0].field.as_deref(), Some("ImagePath"));
    assert_eq!(outcome.undecodable[0].message, MESSAGE);
    assert!(outcome.summary.marked_removed.is_empty());
    assert!(outcome.summary.discarded.is_empty());
    // And it is Failed with ValidationError behind a placeholder, exactly as
    // the boot planner seeds it: reportable, not startable.
    let broken = services.get(BROKEN).expect("a placeholder entry");
    assert_eq!(broken.runtime.state, ServiceState::Failed);
    assert_eq!(broken.runtime.cause, Some(TransitionCause::ValidationError));
    assert!(broken.definition_removed);
    assert_eq!(
        broken.definition.description.as_deref(),
        Some(
            "Service definition failed to decode: MalformedString { field: \"ImagePath\", reason: MissingTerminator }"
        )
    );
}

#[test]
fn an_undecodable_key_of_an_inactive_service_replaces_it_with_the_placeholder() {
    let mut services = service_table(&["app", BROKEN]);
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .undecodable(BROKEN, None, MESSAGE);

    let outcome = reload_config(&mut registry, &mut services).expect("reload");

    assert_eq!(outcome.summary.undecodable, vec![BROKEN]);
    assert!(outcome.summary.discarded.is_empty());
    let broken = services.get(BROKEN).expect("placeholder entry");
    assert_eq!(broken.runtime.state, ServiceState::Failed);
    assert_eq!(broken.runtime.cause, Some(TransitionCause::ValidationError));
    assert!(broken.definition_removed);
    assert!(broken.definition.disabled);
}

/// A running service cannot be failed — there is a process to supervise —
/// so a key that stops decoding underneath it is treated as a withdrawn
/// definition: the instance runs on, the entry is marked definition-removed,
/// and it is discarded when it drains.
#[test]
fn an_undecodable_key_of_a_running_service_marks_its_definition_removed() {
    let mut services = service_table(&["app", BROKEN]);
    services
        .transition_service(
            BROKEN,
            ServiceTransition {
                to: ServiceState::Starting,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("start broken");
    services
        .transition_service(
            BROKEN,
            ServiceTransition {
                to: ServiceState::Active,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("activate broken");
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .undecodable(BROKEN, None, MESSAGE);

    let outcome = reload_config(&mut registry, &mut services).expect("reload");

    assert_eq!(outcome.summary.undecodable, vec![BROKEN]);
    assert!(outcome.summary.marked_removed.is_empty());
    let broken = services.get(BROKEN).expect("retained entry");
    assert_eq!(broken.runtime.state, ServiceState::Active);
    assert!(broken.definition_removed);
    assert_eq!(broken.definition.image_path, "/sbin/broken");
}

/// The dependent of an undecodable key is not a missing-hard-dependency
/// finding that refuses the reload: it loads, and fails through the ordinary
/// propagation from its Failed target when it is next asked to start.
#[test]
fn a_dependent_of_an_undecodable_key_loads_and_is_blocked_when_started() {
    let mut services = service_table(&["app"]);
    let mut dependent = service("dependent", "/sbin/dependent");
    dependent.requires.push(BROKEN.to_string());
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app"), dependent])
        .undecodable(BROKEN, None, MESSAGE);

    let outcome = reload_config(&mut registry, &mut services).expect("the reload is not refused");

    assert_eq!(outcome.summary.added, vec!["dependent"]);
    assert_eq!(outcome.summary.undecodable, vec![BROKEN]);
    assert_eq!(
        services.get("dependent").expect("dependent").runtime.state,
        ServiceState::Inactive
    );

    let plan = plan_on_demand_start(&services, "dependent").expect("plan the start");
    // The placeholder holds no definition to start from, so the dependent
    // is blocked exactly as it is on any withdrawn hard dependency.
    assert_eq!(
        plan.blocked,
        vec![StartPlanBlockedService {
            service: "dependent".to_string(),
            reason: StartBlockReason::HardDependencyUnavailable {
                target: BROKEN.to_string(),
                kind: ServiceDependencyKind::Requires,
                availability: DependencyAvailability::DefinitionRemoved,
            },
        }]
    );
    assert!(plan.starts.is_empty());
}

/// A repaired key restores the service on the next reload, as it does after
/// a boot that blocked it.
#[test]
fn a_reload_that_repairs_the_key_restores_the_service() {
    let mut services = service_table(&["app"]);
    reload_config(
        &mut StaticRegistry::services(vec![service("app", "/sbin/app")])
            .undecodable(BROKEN, None, MESSAGE),
        &mut services,
    )
    .expect("reload with the broken key");

    let outcome = reload_config(
        &mut StaticRegistry::services(vec![
            service("app", "/sbin/app"),
            service(BROKEN, "/sbin/broken-fixed"),
        ]),
        &mut services,
    )
    .expect("reload with the key repaired");

    assert_eq!(outcome.summary.restored, vec![BROKEN]);
    assert!(outcome.summary.undecodable.is_empty());
    let restored = services.get(BROKEN).expect("restored entry");
    assert!(!restored.definition_removed);
    assert_eq!(restored.definition.image_path, "/sbin/broken-fixed");
}
