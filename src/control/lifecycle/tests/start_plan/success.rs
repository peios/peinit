use crate::control::lifecycle::{
    plan_on_demand_start, plan_restart_policy_start, plan_timer_start,
};
use crate::operation::OperationSource;
use crate::service::runtime::TransitionCause;

use super::{activate, planned_services, service, table};

#[test]
fn plans_dependencies_before_requested_service() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    app.wants.push("metrics".to_string());
    let mut db = service("db");
    db.requires.push("network".to_string());
    let services = table(vec![app, db, service("network"), service("metrics")]);

    let plan = plan_on_demand_start(&services, "app").expect("start plan");

    assert!(plan.blocked.is_empty());
    assert_eq!(
        planned_services(&plan.starts),
        vec![
            (
                "network",
                OperationSource::DependencyPropagation,
                TransitionCause::DependencyStart,
            ),
            (
                "db",
                OperationSource::DependencyPropagation,
                TransitionCause::DependencyStart,
            ),
            (
                "metrics",
                OperationSource::DependencyPropagation,
                TransitionCause::DependencyStart,
            ),
            (
                "app",
                OperationSource::Admin,
                TransitionCause::ExplicitStart,
            ),
        ]
    );
}

#[test]
fn already_satisfied_dependencies_are_not_restarted() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let mut services = table(vec![app, service("db")]);
    activate(&mut services, "db");

    let plan = plan_on_demand_start(&services, "app").expect("start plan");

    assert_eq!(
        planned_services(&plan.starts),
        vec![(
            "app",
            OperationSource::Admin,
            TransitionCause::ExplicitStart,
        )]
    );
}

#[test]
fn missing_wants_are_ignored() {
    let mut app = service("app");
    app.wants.push("metrics".to_string());
    let services = table(vec![app]);

    let plan = plan_on_demand_start(&services, "app").expect("start plan");

    assert_eq!(
        planned_services(&plan.starts),
        vec![(
            "app",
            OperationSource::Admin,
            TransitionCause::ExplicitStart,
        )]
    );
    assert!(plan.blocked.is_empty());
}

#[test]
fn disabled_hard_dependencies_are_still_planned_but_disabled_wants_are_skipped() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    app.wants.push("metrics".to_string());
    let mut db = service("db");
    db.disabled = true;
    let mut metrics = service("metrics");
    metrics.disabled = true;
    let services = table(vec![app, db, metrics]);

    let plan = plan_on_demand_start(&services, "app").expect("start plan");

    assert_eq!(
        planned_services(&plan.starts),
        vec![
            (
                "db",
                OperationSource::DependencyPropagation,
                TransitionCause::DependencyStart,
            ),
            (
                "app",
                OperationSource::Admin,
                TransitionCause::ExplicitStart,
            ),
        ]
    );
}

#[test]
fn restart_policy_plan_marks_requested_start_with_policy_source_and_cause() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let services = table(vec![app, service("db")]);

    let plan = plan_restart_policy_start(&services, "app").expect("restart plan");

    assert_eq!(
        plan.requested_operation_source,
        OperationSource::RestartPolicy
    );
    assert_eq!(
        plan.requested_transition_cause,
        TransitionCause::RestartPolicy
    );
    assert_eq!(
        planned_services(&plan.starts),
        vec![
            (
                "db",
                OperationSource::DependencyPropagation,
                TransitionCause::DependencyStart,
            ),
            (
                "app",
                OperationSource::RestartPolicy,
                TransitionCause::RestartPolicy,
            ),
        ]
    );
}

#[test]
fn timer_plan_marks_requested_start_with_timer_source_and_cause() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    let services = table(vec![app, service("db")]);

    let plan = plan_timer_start(&services, "app").expect("timer plan");

    assert_eq!(plan.requested_operation_source, OperationSource::Timer);
    assert_eq!(plan.requested_transition_cause, TransitionCause::Timer);
    assert_eq!(
        planned_services(&plan.starts),
        vec![
            (
                "db",
                OperationSource::DependencyPropagation,
                TransitionCause::DependencyStart,
            ),
            ("app", OperationSource::Timer, TransitionCause::Timer),
        ]
    );
}
