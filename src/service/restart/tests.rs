use crate::service::runtime::{ServiceState, TransitionCause};
use crate::service::{RestartPolicy, ServiceDefinition};

use super::{
    RestartEvaluation, RestartEvaluationAction, evaluate_restart_after_failure,
    is_success_exit_code,
};

#[test]
fn on_failure_policy_restarts_crashes_with_exponential_delay() {
    let mut definition = service();
    definition.restart_delay_secs = 3;

    assert_eq!(
        evaluate_restart_after_failure(&definition, TransitionCause::ProcessCrash, Some(1), 2,),
        RestartEvaluation {
            action: RestartEvaluationAction::Backoff {
                cause: TransitionCause::ProcessCrash,
                delay_secs: 12,
                next_consecutive_failures: 3,
            },
        }
    );
}

#[test]
fn restart_delay_is_capped() {
    let mut definition = service();
    definition.restart_delay_secs = 30;

    assert_eq!(
        evaluate_restart_after_failure(&definition, TransitionCause::ProcessCrash, Some(1), 3,)
            .action,
        RestartEvaluationAction::Backoff {
            cause: TransitionCause::ProcessCrash,
            delay_secs: 60,
            next_consecutive_failures: 4,
        }
    );
}

#[test]
fn restart_budget_exhaustion_fails() {
    let mut definition = service();
    definition.restart_max_retries = 2;

    assert_eq!(
        evaluate_restart_after_failure(&definition, TransitionCause::ProcessCrash, Some(1), 2,)
            .action,
        RestartEvaluationAction::Fail {
            cause: TransitionCause::RestartBudgetExhausted,
            state: ServiceState::Failed,
        }
    );
}

#[test]
fn clean_exit_restart_is_always_only() {
    let mut definition = service();
    definition.restart_policy = RestartPolicy::Always;
    assert!(matches!(
        evaluate_restart_after_failure(&definition, TransitionCause::CleanExitRestart, Some(0), 0,)
            .action,
        RestartEvaluationAction::Backoff { .. }
    ));

    definition.restart_policy = RestartPolicy::OnFailure;
    assert!(matches!(
        evaluate_restart_after_failure(&definition, TransitionCause::CleanExitRestart, Some(0), 0,)
            .action,
        RestartEvaluationAction::Fail { .. }
    ));
}

#[test]
fn success_exit_codes_are_not_failures_for_on_failure_policy() {
    let mut definition = service();
    definition.success_exit_codes = vec![2];

    assert_eq!(
        evaluate_restart_after_failure(&definition, TransitionCause::ProcessCrash, Some(2), 0,)
            .action,
        RestartEvaluationAction::Fail {
            cause: TransitionCause::ProcessCrash,
            state: ServiceState::Failed,
        }
    );
    assert!(is_success_exit_code(&definition, 2));
}

fn service() -> ServiceDefinition {
    ServiceDefinition::simple_system_boot("app", "/sbin/app")
}
