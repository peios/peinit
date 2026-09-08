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
