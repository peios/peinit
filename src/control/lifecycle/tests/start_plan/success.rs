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

// PEI-366. §3.1 says `Disabled` "MUST NOT be started by any trigger" and "MAY
// still be started explicitly via the control interface" — and says nothing
// about being somebody else's `Requires` target. The two paths disagreed:
// boot blocked the dependent, on-demand started the disabled service as a
// dependency.
//
// So an administrator could disable a service to stop it running and have it
// run anyway the next time anyone started something that depends on it —
// exactly the outcome the flag exists to prevent, by a route its description
// does not consider. Nobody started it explicitly; something that requires it
// did. Both paths now block the dependent.
//
// A disabled `Wants` target has always been skipped, and still is: a soft
// dependency is advisory, so there is nothing to block.
#[test]
fn a_disabled_hard_dependency_blocks_an_on_demand_start_as_it_does_a_boot() {
    let mut app = service("app");
    app.requires.push("db".to_string());
    app.wants.push("metrics".to_string());
    let mut db = service("db");
    db.disabled = true;
    let mut metrics = service("metrics");
    metrics.disabled = true;
    let services = table(vec![app, db, metrics]);

    let plan = plan_on_demand_start(&services, "app").expect("start plan");

    assert!(
        plan.starts.is_empty(),
        "an on-demand start overrode an administrator's Disabled: {:?}",
        planned_services(&plan.starts),
    );
    assert_eq!(plan.blocked.len(), 1);
    assert_eq!(plan.blocked[0].service, "app");
    assert_eq!(
        plan.blocked[0].reason,
        crate::control::lifecycle::StartBlockReason::HardDependencyUnavailable {
            target: "db".to_string(),
            kind: crate::service::ServiceDependencyKind::Requires,
            availability: crate::control::lifecycle::DependencyAvailability::Disabled,
        },
    );
}

/// Starting the disabled service *itself* is still allowed — that is what
/// §3.1's "MAY still be started explicitly" means, and it is the escape hatch
/// this change leaves intact.
#[test]
fn a_disabled_service_can_still_be_started_explicitly() {
    let mut db = service("db");
    db.disabled = true;
    let services = table(vec![db]);

    let plan = plan_on_demand_start(&services, "db").expect("start plan");

    assert_eq!(
        planned_services(&plan.starts),
        vec![(
            "db",
            OperationSource::Admin,
            TransitionCause::ExplicitStart,
        )],
    );
    assert!(plan.blocked.is_empty());
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
