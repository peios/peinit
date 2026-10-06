//! peinit's events: their names, their payloads, and the emission policy
//! asked before each is built. `peinit.evman` describes every type and
//! field written here (PGSS §6.10).

mod audit;
mod collector;
mod encode;
mod labels;
mod payload;
pub mod types;

pub use audit::{
    DroppedEvent, GraphPhase, KmesEventSubject, collect_init_recovery_events,
    dropped_event_message, encode_boot_blocked_service_event, encode_config_reload_applied_event,
    encode_critical_failure_event, encode_event_dropped_event,
    encode_graph_validation_error_event, encode_graph_validation_warning_event,
    encode_leaked_cgroup_event, encode_leaked_job_cgroup_event,
    encode_on_failure_loop_suppressed_event, encode_registry_reload_deferred_event,
    encode_reload_undecodable_service_event, encode_reload_unconfirmed_event,
    encode_safe_mode_downgrade_event, encode_service_internal_error_event,
    encode_shutdown_abandoned_event, kmes_event_subject,
};
pub use collector::EventCollector;
pub use encode::{
    MAX_JOB_ENDED_ARGUMENTS_BYTES, NotifyRejectionReason, encode_fd_store_rejection_event,
    encode_graph_event, encode_job_event, encode_job_status_event, encode_notify_field_event,
    encode_notify_progress_event, encode_notify_rejection_event, encode_operation_event,
    encode_output_dropped_event, job_event_type, notify_field_event_type, operation_event_type,
};

#[cfg(test)]
mod tests;
