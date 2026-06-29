use crate::control::lifecycle::{
    DependencyAvailability, OnDemandStartPlanError, StartBlockReason, StartPlanBlockedService,
    plan_on_demand_start,
};
use crate::service::ServiceDependencyKind;

use super::{service, table};

#[test]
fn missing_hard_dependency_blocks_requested_service_without_planned_starts() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let services = table(vec![app]);

    let plan = plan_on_demand_start(&services, "app").expect("blocked plan");

    assert!(plan.starts.is_empty());
    assert_eq!(
        plan.blocked,
        vec![StartPlanBlockedService {
            service: "app".to_string(),
            reason: StartBlockReason::HardDependencyUnavailable {
                target: "db".to_string(),
                kind: ServiceDependencyKind::Requires,
                availability: DependencyAvailability::Missing,
            },
        }]
    );
}

#[test]
fn blocked_dependency_propagates_through_hard_edges() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut db = service("db");
    db.binds_to.push("network".to_string());
    let services = table(vec![app, db]);

    let plan = plan_on_demand_start(&services, "app").expect("blocked plan");

    assert!(plan.starts.is_empty());
    assert_eq!(
        plan.blocked,
        vec![
            StartPlanBlockedService {
                service: "app".to_string(),
                reason: StartBlockReason::HardDependencyBlocked {
                    target: "db".to_string(),
                    kind: ServiceDependencyKind::Requires,
                },
            },
            StartPlanBlockedService {
                service: "db".to_string(),
                reason: StartBlockReason::HardDependencyUnavailable {
                    target: "network".to_string(),
                    kind: ServiceDependencyKind::BindsTo,
                    availability: DependencyAvailability::Missing,
                },
            },
        ]
    );
}

#[test]
fn cycles_in_startable_graph_are_rejected() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut db = service("db");
    db.requires.push("app".to_string());
    let services = table(vec![app, db]);

    let error = plan_on_demand_start(&services, "app").expect_err("cycle");

    assert_eq!(
        error,
        OnDemandStartPlanError::Cycle {
            services: vec!["app".to_string(), "db".to_string()],
        }
    );
}
