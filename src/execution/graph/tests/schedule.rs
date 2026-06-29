use super::*;

#[test]
fn boot_context_releases_roots_then_dependents() {
    let ids = operation_ids(2);
    let jobs = job_ids(2);
    let registry = service("registry");
    let mut app = service("app");
    app.requires.push("registry".to_string());
    let plan = boot_plan(vec![
        prepared_start("registry", ids[0], jobs[0], StartCause::ExplicitStart),
        prepared_start("app", ids[1], jobs[1], StartCause::DependencyStart),
    ]);
    let mut store = GraphExecutionStore::new();
    let context_id = store
        .create_boot_context(&plan, &[registry, app])
        .expect("boot context");

    let first = store.release_ready(context_id, 10).expect("first release");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].service, "app");
    assert_eq!(first[0].operation_id, ids[1]);
    assert_eq!(first[0].action, ReadyGraphOperationAction::PreStartCheck);

    store
        .apply_pre_start_check_passed(ids[1])
        .expect("app precheck passed");
    let dependency_check = store
        .release_ready(context_id, 10)
        .expect("dependency precheck release");
    assert_eq!(dependency_check.len(), 1);
    assert_eq!(dependency_check[0].service, "registry");
    assert_eq!(
        dependency_check[0].action,
        ReadyGraphOperationAction::PreStartCheck
    );

    let blocked = store
        .release_ready(context_id, 10)
        .expect("nothing else ready");
    assert!(blocked.is_empty());

    store
        .apply_pre_start_check_passed(ids[0])
        .expect("registry precheck passed");
    let dependency_start = store
        .release_ready(context_id, 10)
        .expect("dependency start release");
    assert_eq!(dependency_start.len(), 1);
    assert_eq!(dependency_start[0].service, "registry");
    assert_eq!(dependency_start[0].action, ReadyGraphOperationAction::Start);

    store
        .apply_operation_satisfied(ids[0])
        .expect("registry satisfied");
    let second = store
        .release_ready(context_id, 10)
        .expect("dependent release");
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].service, "app");
    assert_eq!(second[0].action, ReadyGraphOperationAction::Start);
}

#[test]
fn max_parallel_starts_limits_release_batch() {
    let ids = operation_ids(2);
    let jobs = job_ids(2);
    let plan = boot_plan(vec![
        prepared_start("one", ids[0], jobs[0], StartCause::ExplicitStart),
        prepared_start("two", ids[1], jobs[1], StartCause::ExplicitStart),
    ]);
    let mut store = GraphExecutionStore::new();
    let context_id = store
        .create_boot_context(&plan, &[service("one"), service("two")])
        .expect("boot context");

    let first = store.release_ready(context_id, 1).expect("first release");
    let second = store
        .release_ready(context_id, 1)
        .expect("running member consumes slot");

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].service, "one");
    assert!(second.is_empty());
}

#[test]
fn zero_parallel_limit_is_rejected() {
    let ids = operation_ids(1);
    let jobs = job_ids(1);
    let plan = boot_plan(vec![prepared_start(
        "registry",
        ids[0],
        jobs[0],
        StartCause::ExplicitStart,
    )]);
    let mut store = GraphExecutionStore::new();
    let context_id = store
        .create_boot_context(&plan, &[service("registry")])
        .expect("boot context");

    let err = store
        .release_ready(context_id, 0)
        .expect_err("zero limit rejected");

    assert_eq!(err, GraphExecutionError::InvalidMaxParallelStarts);
}
