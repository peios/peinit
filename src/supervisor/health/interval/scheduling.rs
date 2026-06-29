use crate::service::runtime::{ServiceState, ServiceTransitionEvent};
use crate::service::{ServiceDefinition, ServiceTableTransition, ServiceType};
use crate::supervisor::work::SupervisorWork;

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(in crate::supervisor) fn apply_health_scheduling_after_transitions(
    work: &mut SupervisorWork,
    transitions: &[ServiceTableTransition],
    observed_at_ns: u64,
) {
    for transition in transitions {
        apply_health_scheduling_after_transition(work, &transition.event, observed_at_ns);
    }
}

pub(in crate::supervisor) fn apply_health_scheduling_after_post_start(
    work: &mut SupervisorWork,
    service: &str,
    observed_at_ns: u64,
) {
    let Some(runtime) = work.services.runtime(service) else {
        return;
    };
    if runtime.state != ServiceState::Active {
        return;
    }
    schedule_active_service_health(work, service, runtime.generation, observed_at_ns);
}

fn apply_health_scheduling_after_transition(
    work: &mut SupervisorWork,
    event: &ServiceTransitionEvent,
    observed_at_ns: u64,
) {
    if event.to == ServiceState::Active {
        schedule_active_service_health(work, &event.service, event.generation, observed_at_ns);
        return;
    }
    if event.from == ServiceState::Active {
        work.health.cancel_service(&event.service);
    }
}

fn schedule_active_service_health(
    work: &mut SupervisorWork,
    service: &str,
    generation: u64,
    observed_at_ns: u64,
) {
    let Some(definition) = work.services.definition(service) else {
        return;
    };
    let Some(runtime) = work.services.runtime(service) else {
        return;
    };
    if !health_enabled(definition) {
        work.health.cancel_service(service);
        return;
    }
    work.health.schedule_interval(
        service,
        generation,
        runtime.cgroup_generation,
        observed_at_ns.saturating_add(seconds_to_ns(definition.health_check_interval_secs)),
    );
}

pub(super) fn health_enabled(definition: &ServiceDefinition) -> bool {
    definition.service_type == ServiceType::Simple && definition.health_check.is_some()
}

pub(super) fn seconds_to_ns(seconds: u64) -> u64 {
    seconds.saturating_mul(NANOS_PER_SEC)
}
