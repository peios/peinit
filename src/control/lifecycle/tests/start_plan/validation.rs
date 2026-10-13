use crate::control::lifecycle::{OnDemandStartPlanError, plan_on_demand_start};
use crate::service::{ServiceGraphFinding, ServiceTrigger, ServiceType};

use super::{activate, service, table};

#[test]
fn invalid_health_and_timer_definitions_are_rejected_before_dispatch() {
    let mut oneshot = service("bad");
    oneshot.service_type = ServiceType::Oneshot;
    oneshot.health_check = Some("/bin/true".into());
    let mut timing = service("bad");
    timing.health_check = Some("/bin/true".into());
    timing.health_check_retries = 3;
    timing.health_check_interval_secs = 10;
    timing.restart_window_secs = 30;
    let mut timer = service("bad");
    timer.triggers = vec![ServiceTrigger::Timer {
        schedule: "not a schedule".into(),
    }];
    for (index, bad) in [oneshot, timing, timer].into_iter().enumerate() {
        for dependency in [false, true] {
            let mut app = service("app");
            app.requires.push("bad".into());
            let services = table(vec![app, bad.clone()]);
            let requested = if dependency { "app" } else { "bad" };
            let OnDemandStartPlanError::Validation { findings } =
                plan_on_demand_start(&services, requested).expect_err("invalid definition")
            else {
                panic!("expected definition validation");
            };
            assert!(findings.iter().any(|finding| match (index, finding) {
                (0, ServiceGraphFinding::UnschedulableHealthCheck { service, .. })
                | (1, ServiceGraphFinding::InvalidHealthCheckRestartWindow { service, .. })
                | (2, ServiceGraphFinding::InvalidTimerSchedule { service, .. }) =>
                    service == "bad",
                _ => false,
            }));
        }
    }
}

#[test]
fn unrelated_and_satisfied_dependencies_do_not_expand_the_validation_scope() {
    let mut bad = service("bad");
    bad.service_type = ServiceType::Oneshot;
    bad.health_check = Some("/bin/true".into());
    let app = service("app");
    let mut services = table(vec![app.clone(), bad.clone()]);
    assert!(plan_on_demand_start(&services, "app").is_ok());
    let mut app = app;
    app.requires.push("bad".into());
    services = table(vec![app, bad]);
    activate(&mut services, "bad");
    let plan = plan_on_demand_start(&services, "app").expect("already satisfied dependency");
    assert_eq!(plan.starts.len(), 1);
    assert_eq!(plan.starts[0].service, "app");
}

#[test]
fn internal_start_sources_do_not_gain_administrator_validation_refusals() {
    use crate::control::lifecycle::{
        plan_binds_to_recovery_start, plan_on_failure_start, plan_restart_policy_start,
        plan_timer_start, plan_tty_release_start,
    };
    let mut bad = service("bad");
    bad.health_check = Some("/bin/true".into());
    bad.health_check_retries = 3;
    bad.health_check_interval_secs = 10;
    bad.restart_window_secs = 30;
    let services = table(vec![bad]);
    for planner in [
        plan_binds_to_recovery_start,
        plan_on_failure_start,
        plan_restart_policy_start,
        plan_timer_start,
        plan_tty_release_start,
    ] {
        let plan = planner(&services, "bad").expect("internal start keeps existing behavior");
        assert_eq!(plan.starts.len(), 1);
        assert_eq!(plan.starts[0].service, "bad");
    }
}
