use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::execution::notify::{AuthenticatedNotifySender, NotifyAppliedField};
use crate::supervisor::SupervisorFdStoreRejectionDispatch;

use super::super::labels::fd_store_outcome_label;
use super::super::payload::{
    finish_event, write_optional_str_field, write_optional_string_field, write_optional_u32_field,
    write_str_field, write_uint_field,
};

pub fn encode_notify_applied_field_events(
    sender: &AuthenticatedNotifySender,
    fields: &[NotifyAppliedField],
) -> Result<Vec<KmesEvent>, BoundaryError> {
    fields
        .iter()
        .filter_map(|field| notify_field_event(sender, field))
        .collect()
}

pub fn encode_notify_rejection_event(
    sender_pid: Option<u32>,
    reason: &str,
    attribution: Option<&AuthenticatedNotifySender>,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(6);
    write_optional_u32_field(&mut writer, "sender_pid", sender_pid);
    write_str_field(&mut writer, "reason", reason);
    match attribution {
        Some(sender) => {
            write_str_field(&mut writer, "service", &sender.service);
            write_str_field(&mut writer, "job_id", &sender.job_id.to_string());
            write_optional_string_field(
                &mut writer,
                "operation_id",
                sender.operation_id.map(|id| id.to_string()),
            );
            write_uint_field(&mut writer, "generation", sender.generation);
        }
        None => {
            write_optional_str_field(&mut writer, "service", None);
            write_optional_str_field(&mut writer, "job_id", None);
            write_optional_str_field(&mut writer, "operation_id", None);
            writer.write_str("generation").write_nil();
        }
    }
    finish_event("notify.rejected", writer)
}

pub fn encode_fd_store_rejection_event(
    rejection: &SupervisorFdStoreRejectionDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(4);
    write_str_field(&mut writer, "service", &rejection.service);
    write_str_field(&mut writer, "name", &rejection.name);
    write_str_field(
        &mut writer,
        "outcome",
        fd_store_outcome_label(rejection.outcome),
    );
    write_str_field(
        &mut writer,
        "reason",
        fd_store_outcome_label(rejection.outcome),
    );
    finish_event("fd_store.rejected", writer)
}

fn notify_field_event(
    sender: &AuthenticatedNotifySender,
    field: &NotifyAppliedField,
) -> Option<Result<KmesEvent, BoundaryError>> {
    match field {
        NotifyAppliedField::Status { text } => Some(encode_notify_value_event(
            sender,
            "notify.status",
            "status",
            text,
        )),
        NotifyAppliedField::Errno { value } => Some(encode_notify_value_event(
            sender,
            "notify.errno",
            "errno",
            value,
        )),
        NotifyAppliedField::ExitStatus { value } => Some(encode_notify_value_event(
            sender,
            "notify.exit_status",
            "exit_status",
            value,
        )),
        // §10.1 asks for STOPPING=1 to be acknowledged by logging it, and the
        // reason is sharper than it looks. STOPPING=1's only effect is the
        // *absence* of an action — peinit suppresses the SIGTERM — which is
        // the one kind of effect that cannot be inferred from what happened.
        //
        // Without this, a service that was stopping and correctly received no
        // SIGTERM is indistinguishable, after the fact, from a service that
        // should have received one and did not: the first is right, the second
        // is a bug in peinit, and an operator looking at a service that took
        // its full StopTimeout and then got SIGKILLed could not tell which
        // they had (PEI-368).
        //
        // READY=1 and RELOADING=1 need no equivalent: both are observable
        // through the state transitions they cause.
        NotifyAppliedField::Stopping => Some(encode_notify_event(sender, "notify.stopping")),
        _ => None,
    }
}

fn encode_notify_event(
    sender: &AuthenticatedNotifySender,
    event_type: &'static str,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(4);
    write_notify_sender(&mut writer, sender);
    finish_event(event_type, writer)
}

fn encode_notify_value_event(
    sender: &AuthenticatedNotifySender,
    event_type: &'static str,
    value_key: &'static str,
    value: &str,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(5);
    write_notify_sender(&mut writer, sender);
    write_str_field(&mut writer, value_key, value);
    finish_event(event_type, writer)
}

fn write_notify_sender(writer: &mut Writer, sender: &AuthenticatedNotifySender) {
    write_str_field(writer, "service", &sender.service);
    write_str_field(writer, "job_id", &sender.job_id.to_string());
    write_optional_string_field(
        writer,
        "operation_id",
        sender.operation_id.map(|id| id.to_string()),
    );
    write_uint_field(writer, "generation", sender.generation);
}
