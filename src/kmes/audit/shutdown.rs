use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::shutdown::ShutdownFinalizationState;
use crate::supervisor::{
    SupervisorShutdownAbandonedDispatch, SupervisorShutdownFinalizationDispatch,
};

use crate::kmes::labels::{service_state_label, transition_cause_label};
use crate::kmes::payload::{
    finish_event, write_optional_str_field, write_optional_u64_field, write_str_field,
    write_uint_field,
};

pub fn encode_critical_failure_event(
    service: &str,
    trigger: &str,
    observed_at_ns: Option<u64>,
    finalization: &SupervisorShutdownFinalizationDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(6);
    write_str_field(&mut writer, "service", service);
    write_str_field(&mut writer, "trigger", trigger);
    write_optional_u64_field(&mut writer, "observed_at_ns", observed_at_ns);
    write_str_field(&mut writer, "final_action", "reboot");
    write_str_field(
        &mut writer,
        "finalization_state",
        finalization_state_label(&finalization.finalization),
    );
    match &finalization.finalization {
        ShutdownFinalizationState::Failed { message, .. } => {
            write_str_field(&mut writer, "finalization_detail", message);
        }
        _ => {
            write_optional_str_field(&mut writer, "finalization_detail", None);
        }
    }
    finish_event("critical.failure", writer)
}

pub fn encode_shutdown_abandoned_event(
    abandoned: &SupervisorShutdownAbandonedDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let transition = &abandoned.service_transition.event;
    let mut writer = Writer::new();
    writer.write_map(7);
    write_str_field(&mut writer, "service", &abandoned.service);
    write_str_field(&mut writer, "cgroup_id", &abandoned.cgroup_id);
    write_str_field(
        &mut writer,
        "from_state",
        service_state_label(transition.from),
    );
    write_str_field(&mut writer, "to_state", service_state_label(transition.to));
    write_str_field(
        &mut writer,
        "cause",
        transition_cause_label(transition.cause),
    );
    write_uint_field(&mut writer, "generation", transition.generation);
    write_str_field(
        &mut writer,
        "message",
        "service cgroup remained populated after SIGKILL and post-kill timeout",
    );
    finish_event("shutdown.abandoned", writer)
}

fn finalization_state_label(state: &ShutdownFinalizationState) -> &'static str {
    match state {
        ShutdownFinalizationState::WaitingForServices => "waiting_for_services",
        ShutdownFinalizationState::Ready => "ready",
        ShutdownFinalizationState::Failed { .. } => "failed",
        ShutdownFinalizationState::Completed => "completed",
    }
}
