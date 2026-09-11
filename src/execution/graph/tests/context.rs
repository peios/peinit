use super::*;

#[test]
fn on_demand_context_associates_returned_operation_ids() {
    let ids = operation_ids(3);
    let mut db = service("db");
    db.triggers.clear();
    let mut app = service("app");
    app.triggers.clear();
    app.requires.push("db".to_string());
    let services = ServiceTable::from_boot_snapshot(vec![db, app]).expect("service table");
    let dispatch = on_demand_dispatch(merged_operation_outcome(ids[0], ids[2]), ids[1]);
    let mut store = GraphExecutionStore::new();

    let context_id = store
        .create_on_demand_context(&dispatch, &services)
        .expect("on demand context");

    assert_eq!(store.associated_contexts(ids[0]), vec![context_id]);
    assert!(store.associated_contexts(ids[2]).is_empty());
    assert_eq!(store.associated_contexts(ids[1]), vec![context_id]);
    let context = store.context(context_id).expect("context");
    assert_eq!(
        context.kind,
        GraphContextKind::OnDemand {
            requested_service: "app".to_string(),
            requested_operation_id: ids[1],
        }
    );
    assert_eq!(
        context.members.get("db").expect("db").status,
        GraphMemberStatus::Running
    );
}

/// An on-demand start of `requested`, which requires `db`, with `db`'s
/// operation outcome as given.
fn dependent_start_dispatch(
    requested: &str,
    dependency: OperationRequestOutcome,
    requested_id: OperationId,
) -> OnDemandStartDispatch {
    let mut dispatch = on_demand_dispatch(dependency, requested_id);
    dispatch.plan.requested = requested.to_string();
    dispatch.plan.starts[1].service = requested.to_string();
    dispatch
}

/// §7.3: a context is retired once every member is terminal, and not a
/// moment before; its operation associations go with it, except where an
/// operation is also associated with a context that is still live.
///
/// Two administrators start `app` and `web`, both of which require `db`,
/// so the second start merges into the first one's `db` operation and
/// that one operation is associated with both contexts.
#[test]
fn a_context_is_retired_only_once_every_member_is_terminal() {
    // db, app, web, and web's own request for db that merged into db's.
    let ids = operation_ids(4);
    let mut db = service("db");
    db.triggers.clear();
    let mut app = service("app");
    app.triggers.clear();
    app.requires.push("db".to_string());
    let mut web = service("web");
    web.triggers.clear();
    web.requires.push("db".to_string());
    let services = ServiceTable::from_boot_snapshot(vec![db, app, web]).expect("service table");
    let mut store = GraphExecutionStore::new();

    let app_context = store
        .create_on_demand_context(
            &dependent_start_dispatch(
                "app",
                operation_outcome(ids[0], OperationConflictDecision::CreateNew),
                ids[1],
            ),
            &services,
        )
        .expect("app's context");
    let web_context = store
        .create_on_demand_context(
            &dependent_start_dispatch("web", merged_operation_outcome(ids[0], ids[3]), ids[2]),
            &services,
        )
        .expect("web's context");
    assert_eq!(
        store.associated_contexts(ids[0]),
        vec![app_context, web_context],
        "the shared db operation belongs to both contexts",
    );

    // No member is terminal: nothing to retire.
    assert!(store.retire_drained_contexts().is_empty());

    // db is satisfied in both contexts, but each still has a member
    // running: still nothing to retire.
    store.apply_operation_satisfied(ids[0]).expect("db satisfied");
    assert!(
        store.retire_drained_contexts().is_empty(),
        "a context with a live member is not retired",
    );
    assert_eq!(store.context_count(), 2);

    // app completes, which drains app's context and only app's.
    store.apply_operation_satisfied(ids[1]).expect("app satisfied");
    assert_eq!(store.retire_drained_contexts(), vec![app_context]);
    assert!(store.context(app_context).is_none());
    assert!(store.context(web_context).is_some());
    assert!(
        store.associated_contexts(ids[1]).is_empty(),
        "app's operation association went with its context",
    );
    assert_eq!(
        store.associated_contexts(ids[0]),
        vec![web_context],
        "db's association survives through the context still live",
    );

    // web completes: the last context drains and goes, and nothing is
    // left pointing anywhere.
    store.apply_operation_satisfied(ids[2]).expect("web satisfied");
    assert_eq!(store.retire_drained_contexts(), vec![web_context]);
    assert_eq!(store.context_count(), 0);
    assert_eq!(store.association_count(), 0);
}

#[test]
fn on_demand_context_accepts_disabled_hard_dependency_members() {
    let ids = operation_ids(2);
    let mut db = service("db");
    db.disabled = true;
    db.triggers.clear();
    let mut app = service("app");
    app.triggers.clear();
    app.requires.push("db".to_string());
    let services = ServiceTable::from_boot_snapshot(vec![db, app]).expect("service table");
    let dispatch = on_demand_dispatch(
        operation_outcome(ids[0], OperationConflictDecision::CreateNew),
        ids[1],
    );
    let mut store = GraphExecutionStore::new();

    let context_id = store
        .create_on_demand_context(&dispatch, &services)
        .expect("disabled hard dependency has a retained definition");

    let context = store.context(context_id).expect("context");
    assert_eq!(context.dependencies.len(), 1);
    assert_eq!(context.dependencies[0].target, "db");
}

#[test]
fn failed_context_build_does_not_consume_context_id() {
    let ids = operation_ids(1);
    let jobs = job_ids(1);
    let plan = boot_plan(vec![prepared_start(
        "missing",
        ids[0],
        jobs[0],
        StartCause::ExplicitStart,
    )]);
    let mut store = GraphExecutionStore::new();

    let err = store
        .create_boot_context(&plan, &[])
        .expect_err("missing definition");
    assert_eq!(
        err,
        GraphContextBuildError::MissingServiceDefinition {
            service: "missing".to_string()
        }
    );

    let context_id = store
        .create_boot_context(&plan, &[service("missing")])
        .expect("first successful context");
    assert_eq!(context_id.as_u64(), 0);
}

#[test]
fn malformed_on_demand_dispatch_without_dependency_outcome_is_rejected() {
    let ids = operation_ids(1);
    let db = service("db");
    let mut app = service("app");
    app.requires.push("db".to_string());
    let services = ServiceTable::from_boot_snapshot(vec![db, app]).expect("service table");
    let mut dispatch = on_demand_dispatch(
        operation_outcome(ids[0], OperationConflictDecision::CreateNew),
        ids[0],
    );
    dispatch.dependency_operations.clear();
    let mut store = GraphExecutionStore::new();

    let err = store
        .create_on_demand_context(&dispatch, &services)
        .expect_err("malformed dispatch");

    assert_eq!(
        err,
        GraphContextBuildError::MissingDependencyOperationOutcome {
            service: "db".to_string()
        }
    );
}

#[test]
fn boot_context_keeps_blocked_members_terminal() {
    let ids = operation_ids(2);
    let jobs = job_ids(1);
    let mut app = service("app");
    app.requires.push("missing".to_string());
    let plan = Phase2BootPlan {
        safe_mode_downgrade: Vec::new(),
        mode: BootMode::Full,
        observed_at_ns: OBSERVED_AT_NS,
        max_parallel_starts: 10,
        starts: vec![prepared_start(
            "registry",
            ids[0],
            jobs[0],
            StartCause::ExplicitStart,
        )],
        blocked: vec![BlockedService {
            service: "app".to_string(),
            operation_id: ids[1],
            reason: BlockedReason::HardDependencyUnavailable {
                target: "missing".to_string(),
                kind: DependencyKind::Requires,
            },
            additional_reasons: Vec::new(),
        }],
        warnings: Vec::new(),
    };
    let mut store = GraphExecutionStore::new();

    let context_id = store
        .create_boot_context(&plan, &[service("registry"), app])
        .expect("boot context");

    let context = store.context(context_id).expect("context");
    assert_eq!(
        context.members.get("app").expect("app").status,
        GraphMemberStatus::Failed
    );
}
