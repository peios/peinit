//! Submitted jobs — arbitrary supervised processes submitted on the jobs
//! socket (PSPU §7).
//!
//! A submitted job is a `JobRecord` of type `Submitted` plus the state this
//! module keeps alongside it: who submitted it, the identity it runs as, the
//! definition it was submitted with, its Security Descriptor, the progress it
//! has reported, the deadlines peinit holds against it, and — once it is
//! terminal — the outcome, retained for the grace period so a submitter
//! polling for the result can still read it.
//!
//! Everything here is pure: the socket, the token operations and the access
//! checks are boundary work and live in `crate::jobs` and `crate::boundary`.

mod definition;
mod model;
mod progress;
mod security;
mod store;
mod view;

#[cfg(test)]
mod tests;

pub use definition::{
    SubmittedJobDefinition, SubmittedJobDefinitionError, parse_submitted_job_definition,
};
pub use model::{
    DEFAULT_SUBMITTED_JOB_RETENTION_NS, JobIdentity, JobProgress, JobProgressUnit, JobReadiness,
    SubmittedJobCause, SubmittedJobEntry, SubmittedJobOutcome, SubmittedJobStop,
    SubmittedJobStopPhase, SubmittedNotifyField,
};
pub use progress::{ProgressParseError, parse_progress, parse_progress_unit};
pub use security::{
    JobAccess, JobAccessCheckError, JobAccessCheckRequest, JobAccessChecker, JobAccessDecision,
    JobAccessDenied, JobDescriptorError, JobDescriptorFactory, JobSecurityDescriptor,
};
pub use store::{
    SubmittedJobDeadline, SubmittedJobDeadlineKind, SubmittedJobListFilter, SubmittedJobStore,
    SubmittedJobStoreError,
};
pub use view::{JobView, JobViewState, job_view, job_view_json};
