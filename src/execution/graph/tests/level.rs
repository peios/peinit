use super::*;

/// A dispatch whose plan starts only the requested service — the shape the
/// planner produces when every dependency is already active, which is
/// exactly when a level edge's target is not a member.
pub(super) fn single_start_dispatch(requested: &str, id: OperationId) -> OnDemandStartDispatch {
    OnDemandStartDispatch {
        plan: OnDemandStartPlan {
            requested: requested.to_string(),
            requested_operation_source: OperationSource::Admin,
            requested_transition_cause: TransitionCause::ExplicitStart,
            starts: vec![PlannedStart {
                service: requested.to_string(),
                operation_source: OperationSource::Admin,
                transition_cause: TransitionCause::ExplicitStart,
            }],
            blocked: Vec::new(),
        },
        requested_operation: operation_outcome(id, OperationConflictDecision::CreateNew),
        dependency_operations: Vec::new(),
        events: Vec::new(),
    }
}

fn definition_without_triggers(name: &str) -> ServiceDefinition {
    let mut definition = service(name);
    definition.triggers.clear();
    definition
}

#[test]
fn a_requires_level_on_a_non_member_holds_until_published() {
    // The PEI-500 bug: `Requires = ["netd:routed"]` with netd already
    // active. netd is not in the plan (nothing to start), so before level
    // edges the context had no edge at all and the dependent sailed
    // through.
    let ids = operation_ids(1);
    let mut waiter = definition_without_triggers("waiter");
    waiter.requires.push("netd:routed".to_string());
    let netd = definition_without_triggers("netd");
    let services = ServiceTable::from_boot_snapshot(vec![netd, waiter]).expect("service table");
    let dispatch = single_start_dispatch("waiter", ids[0]);
    let mut store = GraphExecutionStore::new();
    let context_id = store
        .create_on_demand_context(&dispatch, &services)
        .expect("context");

    let precheck = store
        .release_ready(context_id, 10, &no_levels)
        .expect("precheck release");
    assert_eq!(precheck.len(), 1);
    assert_eq!(precheck[0].action, ReadyGraphOperationAction::PreStartCheck);
    store
        .apply_pre_start_check_passed(ids[0])
        .expect("precheck passed");

    let held = store
        .release_ready(context_id, 10, &|_, _| LevelProbe::NotYetPublished)
        .expect("held release");
    assert!(held.is_empty(), "unmet level must hold the dependent");

    let released = store
        .release_ready(context_id, 10, &|target, level| {
            assert_eq!(target, "netd");
            assert_eq!(level, "routed");
            LevelProbe::Satisfied
        })
        .expect("released");
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].service, "waiter");
    assert_eq!(released[0].action, ReadyGraphOperationAction::Start);
}

#[test]
fn a_member_target_gates_on_the_level_after_its_own_satisfaction() {
    // The target is started by the same plan and reaches Active before it
    // publishes the level (netd is up well before DHCP finishes). The
    // member edge settling must not release the dependent on its own.
    let ids = operation_ids(2);
    let jobs = job_ids(2);
    let registry = service("registry");
    let mut app = service("app");
    app.requires.push("registry:ready".to_string());
    let plan = boot_plan(vec![
        prepared_start("registry", ids[0], jobs[0], StartCause::ExplicitStart),
        prepared_start("app", ids[1], jobs[1], StartCause::DependencyStart),
    ]);
    let mut store = GraphExecutionStore::new();
    let context_id = store
        .create_boot_context(&plan, &[registry, app])
        .expect("boot context");

    for id in [ids[1], ids[0]] {
        let checks = store
            .release_ready(context_id, 10, &no_levels)
            .expect("precheck release");
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].action, ReadyGraphOperationAction::PreStartCheck);
        store
            .apply_pre_start_check_passed(id)
            .expect("precheck passed");
    }
    let registry_start = store
        .release_ready(context_id, 10, &no_levels)
        .expect("registry start");
    assert_eq!(registry_start.len(), 1);
    assert_eq!(registry_start[0].service, "registry");
    store
        .apply_operation_satisfied(ids[0])
        .expect("registry satisfied");

    let held = store
        .release_ready(context_id, 10, &|_, _| LevelProbe::NotYetPublished)
        .expect("held release");
    assert!(
        held.is_empty(),
        "satisfied member with unmet level must hold"
    );

    let released = store
        .release_ready(context_id, 10, &|_, _| LevelProbe::Satisfied)
        .expect("released");
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].service, "app");
    assert_eq!(released[0].action, ReadyGraphOperationAction::Start);
}

#[test]
fn a_wants_level_holds_only_while_someone_could_publish_it() {
    let ids = operation_ids(1);
    let mut waiter = definition_without_triggers("waiter");
    waiter.wants.push("timed:synchronised".to_string());
    let timed = definition_without_triggers("timed");
    let services = ServiceTable::from_boot_snapshot(vec![timed, waiter]).expect("service table");
    let dispatch = single_start_dispatch("waiter", ids[0]);
    let mut store = GraphExecutionStore::new();
    let context_id = store
        .create_on_demand_context(&dispatch, &services)
        .expect("context");
    let precheck = store
        .release_ready(context_id, 10, &no_levels)
        .expect("precheck release");
    assert_eq!(precheck.len(), 1);
    store
        .apply_pre_start_check_passed(ids[0])
        .expect("precheck passed");

    let held = store
        .release_ready(context_id, 10, &|_, _| LevelProbe::NotYetPublished)
        .expect("held release");
    assert!(held.is_empty(), "a running target may still publish: wait");

    // The target dying is what frees a soft waiter — Wants tolerates the
    // level never arriving, it only refuses to jump the gun on one that is
    // on its way.
    let released = store
        .release_ready(context_id, 10, &|_, _| LevelProbe::Absent)
        .expect("released");
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].service, "waiter");
    assert_eq!(released[0].action, ReadyGraphOperationAction::Start);
}

#[test]
fn level_waiter_lookup_finds_undrained_contexts_only() {
    let ids = operation_ids(1);
    let mut waiter = definition_without_triggers("waiter");
    waiter.requires.push("netd:routed".to_string());
    let netd = definition_without_triggers("netd");
    let services = ServiceTable::from_boot_snapshot(vec![netd, waiter]).expect("service table");
    let dispatch = single_start_dispatch("waiter", ids[0]);
    let mut store = GraphExecutionStore::new();
    let context_id = store
        .create_on_demand_context(&dispatch, &services)
        .expect("context");

    assert_eq!(
        store.contexts_with_level_dependency_on("netd"),
        vec![context_id]
    );
    assert!(store.contexts_with_level_dependency_on("timed").is_empty());

    store
        .apply_operation_satisfied(ids[0])
        .expect("waiter satisfied");
    assert!(
        store.contexts_with_level_dependency_on("netd").is_empty(),
        "a drained context has nothing left to release"
    );
}
