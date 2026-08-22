use crate::boot::BootMode;
use crate::boot::phase2::{BlockedReason, DependencyKind, prepare_phase2_boot_plan};
use crate::ids::OperationIdAllocator;
use crate::operation::OperationState;
use crate::operation::store::{OperationEvent, OperationEventDetail, OperationStore};
use crate::service::ServiceDefinition;

fn event_details(events: &[OperationEvent]) -> Vec<OperationEventDetail> {
    events.iter().map(|event| event.detail.clone()).collect()
}

#[test]
fn dispatch_phase2_boot_plan_requests_startable_operations_in_order() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.requires.push("authd".to_string());
    let mut authd = ServiceDefinition::simple_system_boot("authd", "/sbin/authd");
    authd.triggers.clear();
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = crate::ids::JobIdAllocator::new();
    let plan = prepare_phase2_boot_plan(
        BootMode::Full,
        &[app, authd],
        10,
        1_717_171_717_123_456_789,
        &mut operation_ids,
        &mut job_ids,
    )
    .expect("boot plan");
    let mut store = OperationStore::new();

    let dispatch = store
        .dispatch_phase2_boot_plan(&plan)
        .expect("boot dispatch");

    assert!(dispatch.blocked_operation_ids.is_empty());
    assert_eq!(
        dispatch.start_operation_ids,
        plan.starts
            .iter()
            .map(|start| start.operation_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        dispatch
            .events
            .iter()
            .map(|event| event.service.as_str())
            .collect::<Vec<_>>(),
        vec!["authd", "app"],
    );
    assert_eq!(
        event_details(&dispatch.events),
        vec![
            OperationEventDetail::Requested,
            OperationEventDetail::Requested,
        ]
    );
    assert_eq!(
        store.active_for_service("authd"),
        vec![plan.starts[0].operation_id]
    );
    assert_eq!(
        store.active_for_service("app"),
        vec![plan.starts[1].operation_id]
    );
}

#[test]
fn dispatch_phase2_boot_plan_fails_blocked_operations_before_requesting_starts() {
    let mut blocked = ServiceDefinition::simple_system_boot("blocked", "/sbin/blocked");
    blocked.requires.push("missing".to_string());
    let ready = ServiceDefinition::simple_system_boot("ready", "/sbin/ready");
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = crate::ids::JobIdAllocator::new();
    let plan = prepare_phase2_boot_plan(
        BootMode::Full,
        &[blocked, ready],
        10,
        1_717_171_717_123_456_789,
        &mut operation_ids,
        &mut job_ids,
    )
    .expect("boot plan");
    let mut store = OperationStore::new();

    let dispatch = store
        .dispatch_phase2_boot_plan(&plan)
        .expect("boot dispatch");

    assert_eq!(plan.blocked.len(), 1);
    assert_eq!(
        plan.blocked[0].reason,
        BlockedReason::HardDependencyUnavailable {
            target: "missing".to_string(),
            kind: DependencyKind::Requires,
        }
    );
    assert_eq!(
        dispatch.blocked_operation_ids,
        vec![plan.blocked[0].operation_id]
    );
    assert_eq!(
        dispatch.start_operation_ids,
        vec![plan.starts[0].operation_id]
    );
    assert_eq!(
        dispatch
            .events
            .iter()
            .map(|event| event.service.as_str())
            .collect::<Vec<_>>(),
        vec!["blocked", "blocked", "ready"],
    );
    assert_eq!(
        event_details(&dispatch.events),
        vec![
            OperationEventDetail::Requested,
            OperationEventDetail::Failed {
                duration_ns: 0,
                failure_reason: "DependencyFailure: Requires dependency missing is unavailable"
                    .to_string(),
            },
            OperationEventDetail::Requested,
        ]
    );
    assert_eq!(
        store
            .get(plan.blocked[0].operation_id)
            .expect("blocked operation")
            .state,
        OperationState::Failed,
    );
    assert!(store.active_for_service("blocked").is_empty());
    assert_eq!(
        store.active_for_service("ready"),
        vec![plan.starts[0].operation_id],
    );
}

/// PSD-007 §6.2 requires every finding be logged, not just the primary. The
/// operation failure message is the diagnostic channel on the boot path, so
/// the secondary findings have to appear in it — otherwise retaining them
/// changes nothing an administrator can see.
#[test]
fn blocked_failure_message_enumerates_every_retained_finding() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.requires.push("missing-a".to_string());
    app.requires.push("missing-b".to_string());
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = crate::ids::JobIdAllocator::new();
    let plan = prepare_phase2_boot_plan(
        BootMode::Full,
        &[app],
        10,
        1_717_171_717_123_456_789,
        &mut operation_ids,
        &mut job_ids,
    )
    .expect("boot plan");
    let mut store = OperationStore::new();

    let dispatch = store
        .dispatch_phase2_boot_plan(&plan)
        .expect("boot dispatch");

    let failure = dispatch
        .events
        .iter()
        .find_map(|event| match &event.detail {
            OperationEventDetail::Failed { failure_reason, .. } => Some(failure_reason.clone()),
            _ => None,
        })
        .expect("a failure event");

    assert_eq!(
        failure,
        "DependencyFailure: Requires dependency missing-a is unavailable \
         (also: DependencyFailure: Requires dependency missing-b is unavailable) \
         [2 findings]",
    );
}

/// The single-finding case is the common one and its message must not grow a
/// suffix that reads like something was withheld.
#[test]
fn a_single_finding_keeps_its_bare_failure_message() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.requires.push("missing".to_string());
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = crate::ids::JobIdAllocator::new();
    let plan = prepare_phase2_boot_plan(
        BootMode::Full,
        &[app],
        10,
        1_717_171_717_123_456_789,
        &mut operation_ids,
        &mut job_ids,
    )
    .expect("boot plan");
    let mut store = OperationStore::new();

    let dispatch = store
        .dispatch_phase2_boot_plan(&plan)
        .expect("boot dispatch");

    let failure = dispatch
        .events
        .iter()
        .find_map(|event| match &event.detail {
            OperationEventDetail::Failed { failure_reason, .. } => Some(failure_reason.clone()),
            _ => None,
        })
        .expect("a failure event");

    assert_eq!(
        failure,
        "DependencyFailure: Requires dependency missing is unavailable",
    );
}
