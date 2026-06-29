use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceDefinition, ServiceTable};

use super::{ShutdownIgnoredService, plan_graceful_shutdown};

#[test]
fn stop_waves_are_reverse_hard_dependency_order() {
    let db = service("db");
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut ui = service("ui");
    ui.binds_to.push("app".to_string());
    let mut metrics = service("metrics");
    metrics.wants.push("db".to_string());
    let mut services = table(vec![db, app, ui, metrics]);
    for service in ["db", "app", "ui", "metrics"] {
        activate(&mut services, service);
    }

    let plan = plan_graceful_shutdown(&services).expect("shutdown plan");

    assert_eq!(
        wave_services(&plan),
        vec![
            vec!["metrics".to_string(), "ui".to_string()],
            vec!["app".to_string()],
            vec!["db".to_string()],
        ],
    );
}

#[test]
fn classification_matches_shutdown_state_rules() {
    let mut services = table(vec![
        service("active"),
        service("reloading"),
        service("stopping"),
        service("completed"),
        service("starting"),
        service("failed"),
        service("inactive"),
    ]);
    activate(&mut services, "active");
    activate(&mut services, "reloading");
    services
        .transition_service(
            "reloading",
            ServiceTransition {
                to: ServiceState::Reloading,
                cause: TransitionCause::ExplicitReload,
            },
        )
        .expect("reloading");
    activate(&mut services, "stopping");
    services
        .transition_service(
            "stopping",
            ServiceTransition {
                to: ServiceState::Stopping,
                cause: TransitionCause::ExplicitStop,
            },
        )
        .expect("stopping");
    start(&mut services, "completed");
    services
        .transition_service(
            "completed",
            ServiceTransition {
                to: ServiceState::Completed,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("completed");
    start(&mut services, "starting");
    services
        .transition_service(
            "failed",
            ServiceTransition {
                to: ServiceState::Failed,
                cause: TransitionCause::ValidationError,
            },
        )
        .expect("failed");

    let plan = plan_graceful_shutdown(&services).expect("shutdown plan");

    assert_eq!(plan.completed_to_clear, vec!["completed"]);
    assert_eq!(plan.starting_to_kill, vec!["starting"]);
    assert_eq!(
        wave_services(&plan),
        vec![vec![
            "active".to_string(),
            "reloading".to_string(),
            "stopping".to_string(),
        ]],
    );
    assert!(
        plan.stop_waves[0]
            .services
            .iter()
            .any(|participant| participant.service == "stopping" && participant.already_stopping)
    );
    assert_eq!(
        plan.ignored,
        vec![
            ShutdownIgnoredService {
                service: "failed".to_string(),
                state: ServiceState::Failed,
            },
            ShutdownIgnoredService {
                service: "inactive".to_string(),
                state: ServiceState::Inactive,
            },
        ],
    );
}

fn service(name: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"))
}

fn table(definitions: Vec<ServiceDefinition>) -> ServiceTable {
    ServiceTable::from_boot_snapshot(definitions).expect("service table")
}

fn start(services: &mut ServiceTable, service: &str) {
    services
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Starting,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("starting");
}

fn activate(services: &mut ServiceTable, service: &str) {
    start(services, service);
    services
        .transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Active,
                cause: TransitionCause::ExplicitStart,
            },
        )
        .expect("active");
}

fn wave_services(plan: &super::ShutdownPlan) -> Vec<Vec<String>> {
    plan.stop_waves
        .iter()
        .map(|wave| {
            wave.services
                .iter()
                .map(|participant| participant.service.clone())
                .collect()
        })
        .collect()
}
