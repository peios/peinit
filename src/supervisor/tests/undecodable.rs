//! PEI-812: a definition that fails to decode fails that service, not the
//! boot (§2.5).
//!
//! The read side tolerated the bad key and the planner blocked it with
//! `ValidationError`, but the step that applies the blocks looked the name up
//! in a table that — by the planner's own design — had no entry for it, and
//! the `UnknownService` it raised sent the whole boot to recovery.

use crate::control::lifecycle::LifecycleCommandError;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::{Supervisor, SupervisorError, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, LIFECYCLE_COMMAND_NS, ScriptedClock, StaticRegistry, alive_service,
    settings,
};

const BROKEN: &str = "pt-broken";
const MESSAGE: &str = "unclosed quote in ExecStartPre";

fn booted_with_a_broken_key() -> (Supervisor, StaticRegistry) {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry =
        StaticRegistry::services(vec![alive_service("app")]).with_undecodable(BROKEN, MESSAGE);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("one undecodable key does not fail the boot");
    (supervisor, registry)
}

#[test]
fn an_undecodable_definition_fails_only_that_service_and_the_boot_proceeds() {
    let (supervisor, _) = booted_with_a_broken_key();

    // The rest of the graph is unaffected.
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Starting
    );
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);

    // The broken one is reportable: Failed with ValidationError, and marked
    // definition-removed because peinit holds no definition for it.
    let broken = supervisor.service_status(BROKEN).expect("a placeholder entry");
    assert_eq!(broken.state, ServiceState::Failed);
    assert_eq!(broken.cause, Some(TransitionCause::ValidationError));
    assert!(broken.definition_removed);
    assert_eq!(
        broken.description.as_deref(),
        Some("Service definition failed to decode: unclosed quote in ExecStartPre")
    );
    assert!(broken.current_job.is_none());
    assert!(broken.current_operation.is_none());
    // And it is a blocked service in the plan, so it settles at once.
    assert!(supervisor.next_restart_backoff_deadline().is_none());
}

#[test]
fn the_placeholder_refuses_lifecycle_commands_and_leaves_with_its_key() {
    let (mut supervisor, _) = booted_with_a_broken_key();
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS, LIFECYCLE_COMMAND_NS + 1]);

    // There is no definition to start from, and nothing for a reset to
    // clear back to: both are refused as they are for any
    // definition-removed service.
    for command in [
        crate::control::lifecycle::LifecycleCommand::Start,
        crate::control::lifecycle::LifecycleCommand::Reset,
    ] {
        let refused = supervisor
            .run_lifecycle_command(command, BROKEN, None, &mut clock)
            .expect_err("a placeholder accepts no lifecycle command");
        assert!(
            matches!(
                refused,
                SupervisorError::Lifecycle(LifecycleCommandError::DefinitionRemoved {
                    ref service
                }) if service == BROKEN
            ),
            "{command:?}: {refused:?}"
        );
    }

    // The key is deleted and the registry re-read: Failed does not retain
    // a definition-removed entry, so the placeholder is discarded.
    let summary = supervisor
        .services
        .apply_definition_snapshot(vec![alive_service("app")])
        .expect("reload with the key deleted");
    assert_eq!(summary.discarded, vec![BROKEN.to_string()]);
    assert!(supervisor.service_status(BROKEN).is_err());
}

#[test]
fn a_reload_that_repairs_the_key_restores_the_service() {
    let (mut supervisor, _) = booted_with_a_broken_key();

    let repaired = alive_service(BROKEN);
    let summary = supervisor
        .services
        .apply_definition_snapshot(vec![alive_service("app"), repaired.clone()])
        .expect("reload with the key repaired");

    assert_eq!(summary.restored, vec![BROKEN.to_string()]);
    let restored = supervisor.service_status(BROKEN).expect("restored entry");
    assert!(!restored.definition_removed);
    assert_eq!(restored.state, ServiceState::Failed);
    assert_eq!(
        supervisor.services.definition(BROKEN).map(|d| d.image_path.as_str()),
        Some(repaired.image_path.as_str())
    );
    // Failed -> Starting on an explicit start now works as for any service.
    let mut clock = ScriptedClock::new([LIFECYCLE_COMMAND_NS]);
    supervisor
        .start_service(BROKEN, None, &mut clock)
        .expect("the repaired service can be started");
    assert_eq!(
        supervisor.service_status(BROKEN).expect("status").state,
        ServiceState::Starting
    );
}
