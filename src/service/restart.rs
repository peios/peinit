use crate::service::runtime::{RestartConsultation, ServiceState, TransitionCause};

use super::{RestartPolicy, ServiceDefinition};

const MAX_RESTART_DELAY_SECS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartEvaluation {
    pub action: RestartEvaluationAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartEvaluationAction {
    Backoff {
        cause: TransitionCause,
        delay_secs: u64,
        next_consecutive_failures: u32,
    },
    Fail {
        cause: TransitionCause,
        state: ServiceState,
    },
}

pub fn evaluate_restart_after_failure(
    definition: &ServiceDefinition,
    cause: TransitionCause,
    exit_code: Option<i32>,
    consecutive_failures: u32,
) -> RestartEvaluation {
    if cause.restart_consultation() == RestartConsultation::Never {
        return fail(cause);
    }
    if cause == TransitionCause::CleanExitRestart
        && definition.restart_policy != RestartPolicy::Always
    {
        return fail(cause);
    }
    if definition.restart_policy == RestartPolicy::Never {
        return fail(cause);
    }
    if definition.restart_policy == RestartPolicy::OnFailure
        && cause == TransitionCause::ProcessCrash
        && exit_code.is_some_and(|code| is_success_exit_code(definition, code))
    {
        return fail(cause);
    }
    if consecutive_failures >= definition.restart_max_retries {
        return fail(TransitionCause::RestartBudgetExhausted);
    }

    RestartEvaluation {
        action: RestartEvaluationAction::Backoff {
            cause,
            delay_secs: restart_delay_secs(definition.restart_delay_secs, consecutive_failures),
            next_consecutive_failures: consecutive_failures.saturating_add(1),
        },
    }
}

pub fn is_success_exit_code(definition: &ServiceDefinition, exit_code: i32) -> bool {
    exit_code == 0 || definition.success_exit_codes.contains(&exit_code)
}

fn restart_delay_secs(base_delay_secs: u64, consecutive_failures: u32) -> u64 {
    let multiplier = 1_u64.checked_shl(consecutive_failures).unwrap_or(u64::MAX);
    base_delay_secs
        .saturating_mul(multiplier)
        .min(MAX_RESTART_DELAY_SECS)
}

fn fail(cause: TransitionCause) -> RestartEvaluation {
    RestartEvaluation {
        action: RestartEvaluationAction::Fail {
            cause,
            state: ServiceState::Failed,
        },
    }
}

#[cfg(test)]
mod tests;
