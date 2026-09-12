use super::{
    ProcessPresence, RestartConsultation, ServiceRuntimeSnapshot, ServiceState, ServiceTransition,
    ServiceTransitionError, TransitionCause,
};

#[test]
fn states_report_dependent_satisfaction() {
    assert!(ServiceState::Active.satisfies_dependents());
    assert!(ServiceState::Reloading.satisfies_dependents());
    assert!(ServiceState::Completed.satisfies_dependents());
    assert!(ServiceState::Skipped.satisfies_dependents());

    for state in [
        ServiceState::Inactive,
        ServiceState::Starting,
        ServiceState::Stopping,
        ServiceState::Backoff,
        ServiceState::Failed,
        ServiceState::Abandoned,
    ] {
        assert!(!state.satisfies_dependents());
    }
}

#[test]
fn states_report_process_presence() {
    assert_eq!(
        ServiceState::Inactive.process_presence(),
        ProcessPresence::None
    );
    assert_eq!(
        ServiceState::Starting.process_presence(),
        ProcessPresence::Optional
    );
    assert_eq!(
        ServiceState::Active.process_presence(),
        ProcessPresence::Expected
    );
    assert_eq!(
        ServiceState::Abandoned.process_presence(),
        ProcessPresence::Expected
    );
}

#[test]
fn causes_report_restart_consultation_class() {
    assert_eq!(
        TransitionCause::ProcessCrash.restart_consultation(),
        RestartConsultation::RestartEligible,
    );
    assert_eq!(
        TransitionCause::CleanExitRestart.restart_consultation(),
        RestartConsultation::AlwaysOnly,
    );
    assert_eq!(
        TransitionCause::BindsToRecovery.restart_consultation(),
        RestartConsultation::BudgetExempt,
    );
    assert_eq!(
        TransitionCause::DependencyFailure.restart_consultation(),
        RestartConsultation::Never,
    );
}

#[test]
fn on_failure_is_suppressed_for_definition_and_shutdown_causes() {
    assert!(!TransitionCause::ShutdownWave.triggers_on_failure());
    assert!(!TransitionCause::ValidationError.triggers_on_failure());
    assert!(!TransitionCause::CycleDetected.triggers_on_failure());
    assert!(!TransitionCause::DependencyFailure.triggers_on_failure());
    assert!(!TransitionCause::AssertionError.triggers_on_failure());
    assert!(TransitionCause::ProcessCrash.triggers_on_failure());
}

#[test]
fn valid_transition_updates_state_cause_and_generation() {
    let mut snapshot = ServiceRuntimeSnapshot::inactive("svc");
    snapshot.pending_timer = true;

    let event = snapshot
        .transition(ServiceTransition {
            to: ServiceState::Starting,
            cause: TransitionCause::ExplicitStart,
        })
        .expect("start transition");

    assert_eq!(snapshot.state, ServiceState::Starting);
    assert_eq!(snapshot.cause, Some(TransitionCause::ExplicitStart));
    assert_eq!(snapshot.generation, 1);
    assert!(!snapshot.pending_timer);
    assert_eq!(event.from, ServiceState::Inactive);
    assert_eq!(event.to, ServiceState::Starting);
    assert_eq!(event.generation, 1);

    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Active,
            cause: TransitionCause::ExplicitStart,
        })
        .expect("active transition");
    assert_eq!(snapshot.generation, 1);
}

#[test]
fn dependent_satisfaction_timestamp_is_cleared_when_state_no_longer_satisfies() {
    let mut snapshot = ServiceRuntimeSnapshot::inactive("svc");
    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Starting,
            cause: TransitionCause::ExplicitStart,
        })
        .expect("start");
    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Active,
            cause: TransitionCause::ExplicitStart,
        })
        .expect("active");
    snapshot.mark_dependent_satisfied_since(55);
    assert_eq!(snapshot.dependent_satisfied_since_ns, Some(55));

    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Stopping,
            cause: TransitionCause::ExplicitStop,
        })
        .expect("stop");

    assert_eq!(snapshot.dependent_satisfied_since_ns, None);
}

/// §6.1: Reloading satisfies dependents, so a reload is not an outage. The
/// RestartWindow stamp and a published level survive it in both directions.
/// PEI-1079: clearing the stamp here lost the budget reset for the rest of
/// the activation.
#[test]
fn a_reload_keeps_the_satisfaction_stamp_and_the_published_level() {
    let mut snapshot = ServiceRuntimeSnapshot::inactive("svc");
    for (to, cause) in [
        (ServiceState::Starting, TransitionCause::ExplicitStart),
        (ServiceState::Active, TransitionCause::ExplicitStart),
    ] {
        snapshot
            .transition(ServiceTransition { to, cause })
            .expect("start");
    }
    snapshot.mark_dependent_satisfied_since(55);
    snapshot.level = Some("ready".to_string());

    for to in [ServiceState::Reloading, ServiceState::Active] {
        snapshot
            .transition(ServiceTransition {
                to,
                cause: TransitionCause::ExplicitReload,
            })
            .expect("reload leg");
        assert_eq!(
            snapshot.dependent_satisfied_since_ns,
            Some(55),
            "the stamp survives the move to {to:?}",
        );
        assert_eq!(
            snapshot.level.as_deref(),
            Some("ready"),
            "and so does the level"
        );
    }
}

#[test]
fn repeated_start_generation_increments_on_each_starting_transition() {
    let mut snapshot = ServiceRuntimeSnapshot::inactive("oneshot");
    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Starting,
            cause: TransitionCause::ExplicitStart,
        })
        .expect("start");
    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Completed,
            cause: TransitionCause::ExplicitStart,
        })
        .expect("complete");
    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Starting,
            cause: TransitionCause::ExplicitStart,
        })
        .expect("restart completed oneshot");

    assert_eq!(snapshot.generation, 2);
}

#[test]
fn invalid_transition_does_not_mutate_snapshot() {
    let mut snapshot = ServiceRuntimeSnapshot::inactive("svc");
    let before = snapshot.clone();

    let err = snapshot
        .transition(ServiceTransition {
            to: ServiceState::Active,
            cause: TransitionCause::ExplicitStart,
        })
        .expect_err("invalid transition");

    assert_eq!(snapshot, before);
    assert_eq!(
        err,
        ServiceTransitionError::InvalidTransition {
            service: "svc".to_string(),
            from: ServiceState::Inactive,
            to: ServiceState::Active,
            cause: TransitionCause::ExplicitStart,
        }
    );
}

#[test]
fn skipped_start_clears_to_inactive_before_re_evaluation() {
    let mut snapshot = ServiceRuntimeSnapshot::inactive("svc");
    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Skipped,
            cause: TransitionCause::ConditionSkipped,
        })
        .expect("skipped");

    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Inactive,
            cause: TransitionCause::ExplicitStart,
        })
        .expect("clear skipped for explicit start");

    assert_eq!(snapshot.state, ServiceState::Inactive);
    assert_eq!(snapshot.cause, Some(TransitionCause::ExplicitStart));
}

/// PEI-808. A relaunch from Backoff runs the pre-start checks like any other
/// start, and the machine can have changed while the service waited: its
/// terminal taken, a condition no longer met. The check's correct answer used
/// to be an `InvalidTransition` that ended the runtime loop.
#[test]
fn backoff_can_be_skipped_by_a_pre_start_check() {
    for cause in [
        TransitionCause::ConditionSkipped,
        TransitionCause::TtyUnavailable,
    ] {
        let mut snapshot = ServiceRuntimeSnapshot::inactive("svc");
        snapshot
            .transition(ServiceTransition {
                to: ServiceState::Starting,
                cause: TransitionCause::ExplicitStart,
            })
            .expect("start");
        snapshot
            .transition(ServiceTransition {
                to: ServiceState::Backoff,
                cause: TransitionCause::ProcessCrash,
            })
            .expect("backoff");
        let event = snapshot
            .transition(ServiceTransition {
                to: ServiceState::Skipped,
                cause,
            })
            .expect("a relaunch may be skipped");
        assert_eq!(event.from, ServiceState::Backoff);
        assert_eq!(event.to, ServiceState::Skipped);
        assert_eq!(event.cause, cause);
    }
}

/// PEI-808. A relaunch peinit cannot execute fails the service under
/// `InternalError`, and only under that cause: a process failure in Backoff is
/// recorded, not transitioned (§6.2).
#[test]
fn backoff_fails_only_under_internal_error() {
    let mut snapshot = ServiceRuntimeSnapshot::inactive("svc");
    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Starting,
            cause: TransitionCause::ExplicitStart,
        })
        .expect("start");
    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Backoff,
            cause: TransitionCause::ProcessCrash,
        })
        .expect("backoff");
    let before = snapshot.clone();

    snapshot
        .transition(ServiceTransition {
            to: ServiceState::Failed,
            cause: TransitionCause::ProcessCrash,
        })
        .expect_err("a crash does not take Backoff to Failed");
    assert_eq!(snapshot, before);

    let event = snapshot
        .transition(ServiceTransition {
            to: ServiceState::Failed,
            cause: TransitionCause::InternalError,
        })
        .expect("an internal error does");
    assert_eq!(event.from, ServiceState::Backoff);
    assert_eq!(event.to, ServiceState::Failed);
    assert_eq!(
        TransitionCause::InternalError.restart_consultation(),
        RestartConsultation::Never
    );
    assert!(!TransitionCause::InternalError.triggers_on_failure());
}
