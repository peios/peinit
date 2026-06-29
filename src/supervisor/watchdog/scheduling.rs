use crate::service::runtime::{ServiceState, ServiceTransitionEvent};
use crate::service::{ServiceDefinition, ServiceTableTransition, ServiceType};
use crate::supervisor::work::SupervisorWork;

const USEC_PER_SEC: u64 = 1_000_000;

pub(in crate::supervisor) fn apply_watchdog_scheduling_after_transitions(
    work: &mut SupervisorWork,
    transitions: &[ServiceTableTransition],
    observed_at_ns: u64,
) {
    for transition in transitions {
        apply_watchdog_scheduling_after_transition(work, &transition.event, observed_at_ns);
    }
}

pub(in crate::supervisor) fn apply_watchdog_scheduling_after_post_start(
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
    schedule_active_service_watchdog(work, service, runtime.generation, observed_at_ns);
}

fn apply_watchdog_scheduling_after_transition(
    work: &mut SupervisorWork,
    event: &ServiceTransitionEvent,
    observed_at_ns: u64,
) {
    if event.to == ServiceState::Active {
        schedule_active_service_watchdog(work, &event.service, event.generation, observed_at_ns);
        return;
    }
    if event.from == ServiceState::Active {
        work.watchdog.cancel_service(&event.service);
    }
}

fn schedule_active_service_watchdog(
    work: &mut SupervisorWork,
    service: &str,
    generation: u64,
    observed_at_ns: u64,
) {
    let Some(definition) = work.services.definition(service) else {
        work.watchdog.cancel_service(service);
        return;
    };
    let interval_usec = schema_interval_usec(definition);
    if interval_usec == 0 {
        work.watchdog.cancel_service(service);
        return;
    }
    let Some(runtime) = work.services.runtime(service) else {
        work.watchdog.cancel_service(service);
        return;
    };
    work.watchdog.arm(
        service,
        generation,
        runtime.cgroup_generation,
        interval_usec,
        observed_at_ns,
    );
}

fn schema_interval_usec(definition: &ServiceDefinition) -> u64 {
    if definition.service_type != ServiceType::Simple {
        return 0;
    }
    definition
        .watchdog_timeout_secs
        .saturating_mul(USEC_PER_SEC)
}
