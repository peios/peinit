//! PEI-1380: a hold can end before the dependent is ready to start.
//! All boundary implementations here are the in-memory test doubles.

use crate::ids::OperationId;
use crate::operation::OperationState;
use crate::service::ServiceDefinition;
use crate::service::runtime::ServiceState;
use crate::supervisor::work::SupervisorWork;
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::held_level_waiters::{assert_held, datagram};
use super::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, oneshot_service, process, settings,
};

const SECOND: u64 = 1_000_000_000;
const CRASH: u64 = APP_LAUNCH_NS + 1_000;
const RELEASE: u64 = BOOT_NS + 20 * SECOND;
const LEAF_DEADLINE: u64 = RELEASE + 3 * SECOND;

fn held_chain() -> (Supervisor, OperationId) {
    let target = ServiceDefinition::simple_system_boot("target", "/sbin/target");
    // The middle has a longer clock so the leaf can independently expire.
    let mut middle = oneshot_service("middle");
    middle.requires.push("target".to_string());
    middle.start_timeout_secs = 10;
    let mut leaf = oneshot_service("leaf");
    leaf.requires.push("middle".to_string());
    leaf.start_timeout_secs = 3;
    let mut observer = oneshot_service("observer");
    observer.triggers.clear();
    observer.requires.push("leaf".to_string());
    observer.start_timeout_secs = 100;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![target, middle, leaf, observer]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("fake launch")
        .expect("target dispatch");
    let job = supervisor
        .service_status("target")
        .expect("target")
        .current_job
        .expect("job")
        .id;
    supervisor.complete_job(job, CRASH, 1).expect("crash");
    assert_held(&supervisor, "middle");
    let leaf = assert_held(&supervisor, "leaf");
    assert!(
        supervisor
            .process_due_operation_maintenance(RELEASE - SECOND)
            .expect("long hold")
            .operation_timeouts
            .is_empty()
    );
    assert!(!supervisor.operation_timeout_expired(leaf, RELEASE - SECOND));
    (supervisor, leaf)
}

fn relaunch_target(supervisor: &mut Supervisor) {
    assert_eq!(
        supervisor
            .process_due_restart_backoffs(RELEASE - 1_000)
            .expect("restart")
            .len(),
        1
    );
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4243, 10)]);
    let mut clock = ScriptedClock::new([RELEASE - 500]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("fake relaunch")
        .expect("target dispatch");
}

fn mark_target_ready(supervisor: &mut Supervisor) {
    let mut controller = TestProcessController::default();
    supervisor
        .apply_notify_datagram(datagram(4243, b"READY=1"), RELEASE, &mut controller)
        .expect("target ready");
    assert_eq!(
        supervisor.service_status("middle").expect("middle").state,
        ServiceState::Starting
    );
}

fn release_middle(supervisor: &mut Supervisor) {
    relaunch_target(supervisor);
    mark_target_ready(supervisor);
}

#[test]
fn transitive_hold_exit_starts_a_full_pending_lifetime_once() {
    let (mut supervisor, leaf) = held_chain();
    let created = supervisor
        .operations()
        .get(leaf)
        .expect("leaf")
        .created_at_ns;
    release_middle(&mut supervisor);
    let operation = supervisor.operations().get(leaf).expect("same leaf");
    assert_eq!(operation.state, OperationState::Pending);
    assert_eq!(operation.created_at_ns, created);
    assert_eq!(operation.lifetime_from_ns, RELEASE);
    assert!(!supervisor.graph().is_operation_held(leaf));
    assert!(!supervisor.operation_timeout_expired(leaf, RELEASE));

    // An unrelated repeated scheduling pass cannot grant another budget.
    let contexts = supervisor.graph().associated_contexts(leaf);
    let mut work = SupervisorWork::from_supervisor(&supervisor);
    for context in contexts {
        assert!(
            work.release_for_context(context, 10, RELEASE + SECOND)
                .expect("repeat release")
                .is_empty()
        );
    }
    work.commit(&mut supervisor);
    assert_eq!(
        supervisor
            .operations()
            .get(leaf)
            .expect("leaf")
            .lifetime_from_ns,
        RELEASE
    );
    assert!(!supervisor.operation_timeout_expired(leaf, LEAF_DEADLINE - 1));
    assert!(
        supervisor
            .process_due_operation_maintenance(LEAF_DEADLINE - 1)
            .expect("before deadline")
            .operation_timeouts
            .is_empty()
    );
    assert!(supervisor.operation_timeout_expired(leaf, LEAF_DEADLINE));
    let maintenance = supervisor
        .process_due_operation_maintenance(LEAF_DEADLINE)
        .expect("deadline");
    assert_eq!(
        maintenance
            .operation_timeouts
            .iter()
            .map(|event| event.operation_id)
            .collect::<Vec<_>>(),
        vec![leaf]
    );
    assert_eq!(
        supervisor.operations().get(leaf).expect("leaf").state,
        OperationState::Failed
    );
    assert_eq!(
        supervisor.service_status("middle").expect("middle").state,
        ServiceState::Starting
    );
}

#[test]
fn starting_after_a_transitive_hold_exit_does_not_restart_the_clock_again() {
    let (mut supervisor, leaf) = held_chain();
    release_middle(&mut supervisor);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4300, 11), process(4301, 12)]);
    let mut clock = ScriptedClock::new([RELEASE + 1_000, RELEASE + SECOND + 1_000]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("fake middle launch")
        .expect("middle dispatch");
    let middle_job = supervisor
        .service_status("middle")
        .expect("middle")
        .current_job
        .expect("job")
        .id;
    supervisor
        .complete_job(middle_job, RELEASE + SECOND, 0)
        .expect("middle done");
    let operation = supervisor.operations().get(leaf).expect("leaf");
    assert_eq!(operation.state, OperationState::Running);
    assert_eq!(operation.lifetime_from_ns, RELEASE);
    assert_eq!(operation.created_at_ns, BOOT_NS);
    assert!(!supervisor.operation_timeout_expired(leaf, LEAF_DEADLINE - 1));
    assert!(supervisor.operation_timeout_expired(leaf, LEAF_DEADLINE));
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("fake leaf launch")
        .expect("leaf dispatch");
    let leaf_job = supervisor
        .service_status("leaf")
        .expect("leaf")
        .current_job
        .expect("job")
        .id;
    supervisor
        .complete_job(leaf_job, LEAF_DEADLINE - 1, 0)
        .expect("leaf done");
    assert_eq!(
        supervisor.operations().get(leaf).expect("leaf").state,
        OperationState::Completed
    );
}

/// A real on-demand start adopts the held operation as a Running graph proxy.
/// The owning context's wait still controls the single operation's lifetime.
#[test]
fn an_adopted_held_operation_gets_one_lifetime_when_its_owner_releases_it() {
    use crate::execution::graph::GraphMemberStatus;
    let (mut supervisor, leaf) = held_chain();
    relaunch_target(&mut supervisor);
    let mut clock = ScriptedClock::new([RELEASE - 100]);
    supervisor
        .start_service("observer", None, &mut clock)
        .expect("adopt held leaf");
    let contexts = supervisor.graph().associated_contexts(leaf);
    assert_eq!(contexts.len(), 2);
    let statuses = contexts
        .iter()
        .map(|context| {
            supervisor
                .graph()
                .context(*context)
                .expect("context")
                .member_for_operation(leaf)
                .expect("shared leaf")
                .status
        })
        .collect::<Vec<_>>();
    assert!(statuses.contains(&GraphMemberStatus::WaitingForDependencies));
    assert!(statuses.contains(&GraphMemberStatus::Running));
    assert_held(&supervisor, "leaf");
    assert_eq!(
        supervisor
            .operations()
            .get(leaf)
            .expect("leaf")
            .lifetime_from_ns,
        BOOT_NS
    );
    assert!(
        supervisor
            .process_due_operation_maintenance(RELEASE - 1)
            .expect("held despite adoption")
            .operation_timeouts
            .is_empty()
    );

    mark_target_ready(&mut supervisor);
    assert_eq!(
        supervisor
            .operations()
            .get(leaf)
            .expect("leaf")
            .lifetime_from_ns,
        RELEASE
    );
    assert_eq!(
        supervisor
            .operations()
            .get(leaf)
            .expect("leaf")
            .created_at_ns,
        BOOT_NS
    );
    assert_eq!(
        supervisor.operations().get(leaf).expect("leaf").state,
        OperationState::Pending
    );
    assert!(!supervisor.graph().is_operation_held(leaf));
    let mut work = SupervisorWork::from_supervisor(&supervisor);
    for context in contexts {
        assert!(
            work.release_for_context(context, 10, RELEASE + SECOND)
                .expect("repeat adopted release")
                .is_empty()
        );
    }
    work.commit(&mut supervisor);
    assert_eq!(
        supervisor
            .operations()
            .get(leaf)
            .expect("leaf")
            .lifetime_from_ns,
        RELEASE
    );
    assert!(!supervisor.operation_timeout_expired(leaf, LEAF_DEADLINE - 1));
    assert!(supervisor.operation_timeout_expired(leaf, LEAF_DEADLINE));
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4300, 11), process(4301, 12)]);
    let mut clock = ScriptedClock::new([RELEASE + SECOND + 1_000, RELEASE + 2 * SECOND + 1_000]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("fake middle launch")
        .expect("middle dispatch");
    let middle_job = supervisor
        .service_status("middle")
        .expect("middle")
        .current_job
        .expect("job")
        .id;
    let middle_done = supervisor
        .complete_job(middle_job, RELEASE + 2 * SECOND, 0)
        .expect("middle done");
    assert_eq!(
        middle_done
            .start_dispatches
            .iter()
            .filter(|dispatch| dispatch.ready.operation_id == leaf)
            .count(),
        1
    );
    assert_eq!(
        supervisor.operations().get(leaf).expect("same leaf").state,
        OperationState::Running
    );
    assert_eq!(
        supervisor
            .operations()
            .get(leaf)
            .expect("same leaf")
            .lifetime_from_ns,
        RELEASE
    );
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("fake leaf launch")
        .expect("leaf dispatch");
    let leaf_job = supervisor
        .service_status("leaf")
        .expect("leaf")
        .current_job
        .expect("job")
        .id;
    supervisor
        .complete_job(leaf_job, LEAF_DEADLINE - 1, 0)
        .expect("leaf done");
    assert_eq!(
        supervisor.operations().get(leaf).expect("same leaf").state,
        OperationState::Completed
    );
    assert_eq!(
        supervisor
            .service_status("observer")
            .expect("observer")
            .state,
        ServiceState::Starting
    );
    let mut work = SupervisorWork::from_supervisor(&supervisor);
    for context in work.graph.associated_contexts(leaf) {
        assert!(
            work.release_for_context(context, 10, LEAF_DEADLINE)
                .expect("repeat terminal release")
                .is_empty()
        );
    }
    work.commit(&mut supervisor);
    assert_eq!(
        supervisor
            .operations()
            .get(leaf)
            .expect("same leaf")
            .lifetime_from_ns,
        RELEASE
    );
}
