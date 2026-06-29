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

    let root_checks = store.release_ready(context_id, 10).expect("root checks");
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
    let db_check = store.release_ready(context_id, 10).expect("db check");
    assert_eq!(db_check.len(), 1);
    assert_eq!(db_check[0].service, "db");
    assert_eq!(db_check[0].action, ReadyGraphOperationAction::PreStartCheck);
    store
        .apply_pre_start_check_passed(ids[0])
        .expect("db precheck passed");
    let db_start = store.release_ready(context_id, 10).expect("db release");
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
        .release_ready(context_id, 10)
        .expect("wants dependent release");
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].service, "ui");
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
