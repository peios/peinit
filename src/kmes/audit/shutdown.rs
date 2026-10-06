use crate::boundary::{BoundaryError, KmesEvent};
use crate::shutdown::ShutdownFinalizationState;
use crate::supervisor::{
    CriticalRebootTrigger, SupervisorShutdownAbandonedDispatch,
    SupervisorShutdownFinalizationDispatch,
};

use crate::kmes::labels::{service_state_label, transition_cause_label};
use crate::kmes::payload::Payload;
use crate::kmes::types::{CRITICAL_SERVICE_FAILED, SERVICE_ABANDONED};

/// `peinit.critical-service.failed`: a Critical service failed and peinit
/// took the machine to its reboot final action.
pub fn encode_critical_failure_event(
    service: &str,
    trigger: CriticalRebootTrigger,
    finalization: &SupervisorShutdownFinalizationDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("object.service.name", service);
    payload.set("outcome.reason", trigger.kmes_id());
    payload.set(
        "shutdown.finalization-state",
        finalization_state_label(&finalization.finalization),
    );
    if let ShutdownFinalizationState::Failed { message, .. } = &finalization.finalization {
        payload.set("outcome.detail", message.as_str());
    }
    payload.finish(CRITICAL_SERVICE_FAILED)
}

/// `peinit.service.abandoned`: a service's cgroup was still populated after
/// SIGKILL and the post-kill timeout, so peinit gave up on it.
pub fn encode_shutdown_abandoned_event(
    abandoned: &SupervisorShutdownAbandonedDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let transition = &abandoned.service_transition.event;
    let mut payload = Payload::new();
    payload.set("object.service.name", abandoned.service.as_str());
    payload.set("object.cgroup.path", abandoned.cgroup_id.as_str());
    payload.set(
        "object.service.state-previous",
        service_state_label(transition.from),
    );
    payload.set("object.service.state", service_state_label(transition.to));
    payload.set(
        "object.service.transition-cause",
        transition_cause_label(transition.cause),
    );
    payload.set("object.service.generation", transition.generation);
    payload.finish(SERVICE_ABANDONED)
}

fn finalization_state_label(state: &ShutdownFinalizationState) -> &'static str {
    match state {
        ShutdownFinalizationState::WaitingForServices => "waiting-for-services",
        ShutdownFinalizationState::Ready => "ready",
        ShutdownFinalizationState::Failed { .. } => "failed",
        ShutdownFinalizationState::Completed => "completed",
    }
}
