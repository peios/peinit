use super::*;

#[test]
fn required_dependency_failure_propagates_but_wants_do_not() {
    let ids = operation_ids(3);
    let jobs = job_ids(3);
    let db = service("db");
    let mut api = service("api");
    api.requires.push("db".to_string());
    let mut ui = service("ui");
    ui.wants.push("db".to_string());
    let plan = boot_plan(vec![
        prepared_start("db", ids[0], jobs[0], StartCause::ExplicitStart),
        prepared_start("api", ids[1], jobs[1], StartCause::DependencyStart),
        prepared_start("ui", ids[2], jobs[2], StartCause::DependencyStart),
    ]);
    let mut store = GraphExecutionStore::new();
    let context_id = store
        .create_boot_context(&plan, &[db, api, ui])
        .expect("boot context");

    let root_checks = store
        .release_ready(context_id, 10, &no_levels)
        .expect("root checks");
    assert_eq!(
        root_checks
            .iter()
            .map(|ready| (ready.service.as_str(), ready.action))
            .collect::<Vec<_>>(),
        vec![
            ("api", ReadyGraphOperationAction::PreStartCheck),
            ("ui", ReadyGraphOperationAction::PreStartCheck),
        ],
    );
    store
        .apply_pre_start_check_passed(ids[1])
        .expect("api precheck passed");
    store
        .apply_pre_start_check_passed(ids[2])
        .expect("ui precheck passed");
    let db_check = store
        .release_ready(context_id, 10, &no_levels)
        .expect("db check");
    assert_eq!(db_check.len(), 1);
    assert_eq!(db_check[0].service, "db");
    assert_eq!(db_check[0].action, ReadyGraphOperationAction::PreStartCheck);
    store
        .apply_pre_start_check_passed(ids[0])
        .expect("db precheck passed");
    let db_start = store
        .release_ready(context_id, 10, &no_levels)
        .expect("db release");
    assert_eq!(db_start.len(), 1);
    assert_eq!(db_start[0].service, "db");
    assert_eq!(db_start[0].action, ReadyGraphOperationAction::Start);
    let events = store.apply_operation_failed(ids[0]).expect("db failed");

    assert_eq!(
        events
            .iter()
            .map(|event| (event.service.as_str(), event.outcome))
            .collect::<Vec<_>>(),
        vec![
            ("db", GraphTerminalOutcome::Failed),
            ("api", GraphTerminalOutcome::Failed)
        ]
    );
    let ready = store
        .release_ready(context_id, 10, &no_levels)
        .expect("wants dependent release");
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].service, "ui");
}

/// A member held for a restart (PEI-821): its hard dependents stay waiting
/// and its soft ones too — the target is coming back — and the hold is
/// settled by *service*, since the relaunch runs under an operation this
/// context never associated.
#[test]
fn a_member_awaiting_restart_holds_its_dependents_until_settled_by_service() {
    let ids = operation_ids(3);
    let jobs = job_ids(3);
    let db = service("db");
    let mut api = service("api");
    api.requires.push("db".to_string());
    let mut ui = service("ui");
    ui.wants.push("db".to_string());
    let plan = boot_plan(vec![
        prepared_start("db", ids[0], jobs[0], StartCause::ExplicitStart),
        prepared_start("api", ids[1], jobs[1], StartCause::DependencyStart),
        prepared_start("ui", ids[2], jobs[2], StartCause::DependencyStart),
    ]);
    let mut store = GraphExecutionStore::new();
    let context_id = store
        .create_boot_context(&plan, &[db, api, ui])
        .expect("boot context");
    for id in [ids[1], ids[2], ids[0]] {
        store
            .release_ready(context_id, 10, &no_levels)
            .expect("precheck release");
        store
            .apply_pre_start_check_passed(id)
            .expect("precheck passed");
    }
    let db_start = store
        .release_ready(context_id, 10, &no_levels)
        .expect("db release");
    assert_eq!(db_start[0].service, "db");

    let held = store
        .apply_operation_awaiting_restart(ids[0])
        .expect("db held for restart");
    assert_eq!(held, vec![context_id]);
    assert!(store.is_awaiting_restart("db"));
    assert_eq!(store.awaiting_restart_services(), vec!["db".to_string()]);
    assert!(store.is_operation_held(ids[1]), "api's start is held");
    assert!(store.is_operation_held(ids[2]), "ui's start is held");
    assert!(
        !store.is_operation_held(ids[0]),
        "db's own operation is over, not held"
    );
    assert!(
        store
            .release_ready(context_id, 10, &no_levels)
            .expect("held release")
            .is_empty(),
        "neither the Requires nor the Wants dependent moves while db is coming back"
    );
    assert!(!store.context(context_id).expect("context").is_drained());
    assert!(
        store.retire_drained_contexts().is_empty(),
        "a context holding a member is live"
    );

    // The relaunch completing settles the hold by service name.
    let events = store
        .settle_awaiting_restart("db", GraphTerminalOutcome::Satisfied)
        .expect("settled");
    assert_eq!(
        events
            .iter()
            .map(|event| (event.service.as_str(), event.operation_id, event.outcome))
            .collect::<Vec<_>>(),
        vec![("db", ids[0], GraphTerminalOutcome::Satisfied)]
    );
    let ready = store
        .release_ready(context_id, 10, &no_levels)
        .expect("released");
    assert_eq!(
        ready
            .iter()
            .map(|ready| ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["api", "ui"]
    );
    assert!(
        store
            .settle_awaiting_restart("db", GraphTerminalOutcome::Satisfied)
            .expect("nothing left to settle")
            .is_empty()
    );
}

/// A relaunch that fails for good settles the hold on its way through the
/// ordinary terminal path: the earlier context's member fails and its hard
/// dependents with it, while the soft one proceeds.
#[test]
fn a_terminal_operation_settles_the_holds_on_its_service_in_other_contexts() {
    let ids = operation_ids(4);
    let jobs = job_ids(3);
    let db = service("db");
    let mut api = service("api");
    api.requires.push("db".to_string());
    let mut ui = service("ui");
    ui.wants.push("db".to_string());
    let plan = boot_plan(vec![
        prepared_start("db", ids[0], jobs[0], StartCause::ExplicitStart),
        prepared_start("api", ids[1], jobs[1], StartCause::DependencyStart),
        prepared_start("ui", ids[2], jobs[2], StartCause::DependencyStart),
    ]);
    let services = ServiceTable::from_boot_snapshot(vec![db.clone(), api.clone(), ui.clone()])
        .expect("service table");
    let mut store = GraphExecutionStore::new();
    let boot = store
        .create_boot_context(&plan, &[db, api, ui])
        .expect("boot context");
    for id in [ids[1], ids[2], ids[0]] {
        store
            .release_ready(boot, 10, &no_levels)
            .expect("precheck release");
        store
            .apply_pre_start_check_passed(id)
            .expect("precheck passed");
    }
    store
        .release_ready(boot, 10, &no_levels)
        .expect("db release");
    store
        .apply_operation_awaiting_restart(ids[0])
        .expect("db held for restart");

    // The relaunch: its own context, its own operation.
    let relaunch = store
        .create_on_demand_context(&level::single_start_dispatch("db", ids[3]), &services)
        .expect("relaunch context");
    store
        .release_ready(relaunch, 10, &no_levels)
        .expect("relaunch precheck");
    store
        .apply_pre_start_check_passed(ids[3])
        .expect("relaunch precheck passed");
    store
        .release_ready(relaunch, 10, &no_levels)
        .expect("relaunch start");

    let events = store
        .apply_operation_failed(ids[3])
        .expect("relaunch failed for good");
    assert_eq!(
        events
            .iter()
            .map(|event| (event.context_id, event.service.as_str(), event.outcome))
            .collect::<Vec<_>>(),
        vec![
            (relaunch, "db", GraphTerminalOutcome::Failed),
            (boot, "db", GraphTerminalOutcome::Failed),
            (boot, "api", GraphTerminalOutcome::Failed),
        ]
    );
    assert!(!store.is_awaiting_restart("db"));
    let ready = store.release_ready(boot, 10, &no_levels).expect("released");
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].service, "ui", "the soft dependent proceeds");
}

#[test]
fn unassociated_terminal_operation_does_not_emit_graph_events() {
    let id = operation_ids(1)[0];
    let mut store = GraphExecutionStore::new();

    let events = store
        .apply_operation_satisfied(id)
        .expect("unassociated operation is ignored");

    assert!(events.is_empty());
}
