use crate::job::JobEvent;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{
    RestartEvaluationAction, RestartPolicy, ServiceDefinition, ServiceTable, ServiceTableError,
    evaluate_restart_after_failure,
};

use super::ended::EndedJob;
use super::model::{ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError};

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(super) fn apply_active_simple_exit(
    services: &mut ServiceTable,
    service: &str,
    job_event: JobEvent,
    definition: &ServiceDefinition,
    ended: EndedJob,
) -> Result<ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError> {
    if ended.is_success_for(definition) {
        if definition.restart_policy == RestartPolicy::Always {
            return apply_restart_evaluation(
                services,
                service,
                job_event,
                definition,
                ended,
                TransitionCause::CleanExitRestart,
            );
        }
        return apply_active_transition(
            services,
            service,
            job_event,
            ServiceState::Inactive,
            TransitionCause::CleanExit,
        );
    }

    apply_restart_evaluation(
        services,
        service,
        job_event,
        definition,
        ended,
        TransitionCause::ProcessCrash,
    )
}

pub(super) fn apply_reloading_simple_exit(
    services: &mut ServiceTable,
    service: &str,
    job_event: JobEvent,
    definition: &ServiceDefinition,
    ended: EndedJob,
) -> Result<ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError> {
    apply_restart_evaluation(
        services,
        service,
        job_event,
        definition,
        ended,
        TransitionCause::ProcessCrash,
    )
}

fn apply_restart_evaluation(
    services: &mut ServiceTable,
    service: &str,
    job_event: JobEvent,
    definition: &ServiceDefinition,
    ended: EndedJob,
    cause: TransitionCause,
) -> Result<ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError> {
    // §3.5: a service whose definition has been withdrawn is not restarted
    // when its instance exits. `RestartPolicy` is moot — there is no
    // definition left to restart it from — so the evaluation is skipped
    // entirely rather than allowed to reach a state it could never leave.
    //
    // Backoff was the state it could never leave, and the two halves of that
    // deadlock are in different files. `restart_backoff_deadlines` skips
    // definition-removed entries, so nothing moved it out; Backoff is one of
    // the states that keeps an entry alive after removal, so nothing discarded
    // it either. The entry stayed in `status` describing a restart that would
    // never happen, went on refusing every lifecycle command with
    // UNKNOWN_SERVICE, and held its stored descriptors open in PID 1 for the
    // life of the process (PEI-346).
    if definition_removed(services, service) {
        let (state, cause) = match cause {
            // A Simple service exiting zero under `RestartPolicy=Always` is a
            // clean exit that policy would have restarted. Without a policy to
            // apply it is simply a clean exit.
            TransitionCause::CleanExitRestart => {
                (ServiceState::Inactive, TransitionCause::CleanExit)
            }
            cause => (ServiceState::Failed, cause),
        };
        return apply_active_transition(services, service, job_event, state, cause);
    }
    let consecutive_failures = services
        .runtime(service)
        .ok_or_else(|| unknown_service(service))?
        .consecutive_restart_failures;
    let evaluation =
        evaluate_restart_after_failure(definition, cause, ended.exit_code, consecutive_failures);

    match evaluation.action {
        RestartEvaluationAction::Backoff {
            cause, delay_secs, ..
        } => apply_restart_backoff(
            services,
            service,
            job_event,
            cause,
            backoff_until(&ended, delay_secs),
        ),
        RestartEvaluationAction::Fail { cause, state } => {
            apply_active_transition(services, service, job_event, state, cause)
        }
    }
}

fn apply_restart_backoff(
    services: &mut ServiceTable,
    service: &str,
    job_event: JobEvent,
    cause: TransitionCause,
    backoff_until_ns: u64,
) -> Result<ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError> {
    let mut next_services = services.clone();
    let transition = next_services
        .transition_service_to_restart_backoff(service, cause, backoff_until_ns)
        .map_err(ServiceMainJobTerminalError::ServiceTable)?;

    *services = next_services;

    Ok(ServiceMainJobTerminalDispatch {
        job_event,
        operation_events: Vec::new(),
        service_transitions: vec![transition],
        graph_events: Vec::new(),
        post_start_hook: None,
        late_exit: None,
    })
}

fn apply_active_transition(
    services: &mut ServiceTable,
    service: &str,
    job_event: JobEvent,
    to: ServiceState,
    cause: TransitionCause,
) -> Result<ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError> {
    let mut next_services = services.clone();
    let transition = next_services
        .transition_service(service, ServiceTransition { to, cause })
        .map_err(ServiceMainJobTerminalError::ServiceTable)?;

    *services = next_services;

    Ok(ServiceMainJobTerminalDispatch {
        job_event,
        operation_events: Vec::new(),
        service_transitions: vec![transition],
        graph_events: Vec::new(),
        post_start_hook: None,
        late_exit: None,
    })
}

/// The definition behind this service has been withdrawn from the registry
/// and the entry is being kept alive only until its instance drains.
fn definition_removed(services: &ServiceTable, service: &str) -> bool {
    services
        .get(service)
        .is_some_and(|entry| entry.definition_removed)
}

fn backoff_until(ended: &EndedJob, delay_secs: u64) -> u64 {
    ended
        .ended_at_ns
        .saturating_add(delay_secs.saturating_mul(NANOS_PER_SEC))
}

fn unknown_service(service: &str) -> ServiceMainJobTerminalError {
    ServiceMainJobTerminalError::ServiceTable(ServiceTableError::UnknownService {
        service: service.to_string(),
    })
}
