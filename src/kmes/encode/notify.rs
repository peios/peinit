use crate::boundary::{BoundaryError, KmesEvent};
use crate::execution::notify::{AuthenticatedNotifySender, NotifyAppliedField};
use crate::service::runtime::ServiceProgressReport;
use crate::submitted::JobProgressUnit;
use crate::supervisor::SupervisorFdStoreRejectionDispatch;

use super::super::labels::fd_store_outcome_label;
use super::super::payload::Payload;
use super::super::types::{
    FD_STORE_REJECTED, NOTIFY_ERRNO_REPORTED, NOTIFY_EXIT_STATUS_REPORTED,
    NOTIFY_PROGRESS_REPORTED, NOTIFY_REJECTED, NOTIFY_STATUS_REPORTED, NOTIFY_STOPPING_REPORTED,
};

/// The type an applied notification field is recorded as, for the fields
/// that are recorded at all. `READY=1` and `RELOADING=1` are not: both are
/// observable through the state transitions they cause.
///
/// `STOPPING=1` is recorded, and the reason is sharper than it looks: its
/// only effect is the *absence* of an action — peinit suppresses the
/// SIGTERM — which is the one kind of effect that cannot be inferred from
/// what happened. Without the record, a service that was stopping and
/// correctly received no SIGTERM is indistinguishable, after the fact, from
/// a service that should have received one and did not (PEI-368).
pub fn notify_field_event_type(field: &NotifyAppliedField) -> Option<&'static str> {
    match field {
        NotifyAppliedField::Status { .. } => Some(NOTIFY_STATUS_REPORTED),
        NotifyAppliedField::Errno { .. } => Some(NOTIFY_ERRNO_REPORTED),
        NotifyAppliedField::ExitStatus { .. } => Some(NOTIFY_EXIT_STATUS_REPORTED),
        NotifyAppliedField::Stopping => Some(NOTIFY_STOPPING_REPORTED),
        _ => None,
    }
}

/// The record of one applied notification field, for a field
/// [`notify_field_event_type`] names a type for.
pub fn encode_notify_field_event(
    sender: &AuthenticatedNotifySender,
    field: &NotifyAppliedField,
) -> Result<KmesEvent, BoundaryError> {
    let Some(event_type) = notify_field_event_type(field) else {
        return Err(BoundaryError::Kmes(format!(
            "notification field {field:?} is not recorded as an event"
        )));
    };
    let mut payload = Payload::new();
    write_sender(&mut payload, sender);
    match field {
        NotifyAppliedField::Status { text } => {
            payload.set("notify.status", text.as_str());
        }
        NotifyAppliedField::Errno { value } => {
            payload.set_opt("notify.errno", errno_value(value));
        }
        NotifyAppliedField::ExitStatus { value } => {
            payload.set_opt("notify.exit-status", value.parse::<i64>().ok());
        }
        _ => {}
    }
    payload.finish(event_type)
}

/// `ERRNO=` as `int.errno`: the positive number a service sends, negated
/// (PGSS §6.6). Text that is not a positive number has no errno to carry.
fn errno_value(text: &str) -> Option<i64> {
    match text.parse::<i64>() {
        Ok(value) if value > 0 => Some(-value),
        _ => None,
    }
}

/// `peinit.notify.progress.reported`: the progress a service has retained
/// after a datagram that carried `PROGRESS` or `PROGRESS_UNIT`, at most once
/// a second per incarnation (PSPU §4.19, §4.A). The value fields are
/// `peinit.job.status.reported`'s.
pub fn encode_notify_progress_event(
    sender: &AuthenticatedNotifySender,
    report: &ServiceProgressReport,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    write_sender(&mut payload, sender);
    if let Some(progress) = report.progress {
        payload.set("notify.progress.current", progress.current);
        payload.set_opt("notify.progress.total", progress.total);
        payload.set("notify.progress.bounded", progress.bounded);
    }
    payload.set_opt(
        "notify.progress.unit",
        report.unit.map(JobProgressUnit::wire),
    );
    payload.finish(NOTIFY_PROGRESS_REPORTED)
}

/// Why a notification datagram was refused, in
/// `peinit.notify.rejected`'s `outcome.reason` vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyRejectionReason {
    InvalidUtf8,
    MalformedLine,
    Truncated,
    UnauthenticatedSender,
    MissingService,
    JobNotRunning,
    MissingProcess,
    PidfdMismatch,
    ProcessVerificationFailed,
    GenerationMismatch,
    MissingStartOperation,
    MissingReloadOperation,
    UnsupportedReloadOperation,
    UnsupportedReadyState,
    ShutdownRefused,
    InternalError,
}

impl NotifyRejectionReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::InvalidUtf8 => "invalid-utf8",
            Self::MalformedLine => "malformed-line",
            Self::Truncated => "truncated",
            Self::UnauthenticatedSender => "unauthenticated-sender",
            Self::MissingService => "missing-service",
            Self::JobNotRunning => "job-not-running",
            Self::MissingProcess => "missing-process",
            Self::PidfdMismatch => "pidfd-mismatch",
            Self::ProcessVerificationFailed => "process-verification-failed",
            Self::GenerationMismatch => "generation-mismatch",
            Self::MissingStartOperation => "missing-start-operation",
            Self::MissingReloadOperation => "missing-reload-operation",
            Self::UnsupportedReloadOperation => "unsupported-reload-operation",
            Self::UnsupportedReadyState => "unsupported-ready-state",
            Self::ShutdownRefused => "shutdown-refused",
            Self::InternalError => "internal-error",
        }
    }
}

/// `peinit.notify.rejected`: a datagram refused, with whoever peinit could
/// attribute it to.
pub fn encode_notify_rejection_event(
    sender_pid: Option<u32>,
    reason: NotifyRejectionReason,
    attribution: Option<&AuthenticatedNotifySender>,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("outcome.reason", reason.label());
    payload.set_opt("subject.process.pid", sender_pid);
    if let Some(sender) = attribution {
        write_sender(&mut payload, sender);
    }
    payload.finish(NOTIFY_REJECTED)
}

/// `peinit.fd-store.rejected`: a descriptor a service asked peinit to keep
/// was refused.
pub fn encode_fd_store_rejection_event(
    rejection: &SupervisorFdStoreRejectionDispatch,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    payload.set("object.service.name", rejection.service.as_str());
    payload.set("object.fd-store.name", rejection.name.as_str());
    payload.set("outcome.reason", fd_store_outcome_label(rejection.outcome));
    payload.finish(FD_STORE_REJECTED)
}

/// The authenticated sender of a notification: the service, the job whose
/// process sent it, the run of the service, and the operation it served.
fn write_sender(payload: &mut Payload, sender: &AuthenticatedNotifySender) {
    payload.set("subject.service.name", sender.service.as_str());
    payload.set("subject.job.guid", sender.job_id);
    payload.set("subject.job.activation-generation", sender.generation);
    payload.set_opt("subject.operation.guid", sender.operation_id);
}
