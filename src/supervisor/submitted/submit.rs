use std::os::fd::IntoRawFd;

use serde_json::Map;

use crate::boundary::{JobIdentityError, JobIdentityProvider, JobIdentitySource};
use crate::ids::JobId;
use crate::job::{JobRecord, SubmittedJobSpec};
use crate::jobs::connection::JobsPeer;
use crate::jobs::socket::JobsMessage;
use crate::submitted::{
    JobDescriptorError, JobDescriptorFactory, JobIdentity, JobReadiness, SubmittedJobEntry,
    parse_submitted_job_definition,
};

use super::error::JobsCommandError;
use crate::supervisor::dispatch::SupervisorJobSubmitDispatch;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    /// Accept a `submit` (PSPU §7.6): validate, establish the identity,
    /// check the quota, create the record and queue the launch. A refusal
    /// leaves nothing behind — no record, no event, no quota.
    pub fn submit_job<I, D>(
        &mut self,
        peer: &JobsPeer,
        object: &Map<String, serde_json::Value>,
        message: JobsMessage,
        identity_provider: &mut I,
        descriptor_factory: &mut D,
        now_ns: u64,
    ) -> Result<(JobId, SupervisorJobSubmitDispatch), JobsCommandError>
    where
        I: JobIdentityProvider + ?Sized,
        D: JobDescriptorFactory + ?Sized,
    {
        let JobsMessage {
            token, descriptors, ..
        } = message;
        let definition = parse_submitted_job_definition(object, descriptors.len())
            .map_err(JobsCommandError::Definition)?;
        if self.shutdown.is_some() {
            return Err(JobsCommandError::ShuttingDown);
        }

        // 1. Identity. Attached beats peer-primary; there is no third route.
        let source = match token {
            Some(token) => JobIdentitySource::AttachedToken { token },
            None => JobIdentitySource::PeerPrimary {
                pidfd: std::os::fd::AsRawFd::as_raw_fd(&peer.pidfd),
            },
        };
        let prepared =
            identity_provider
                .prepare_job_identity(source)
                .map_err(|error| match error {
                    JobIdentityError::BadToken(reason) => JobsCommandError::BadToken(reason),
                    JobIdentityError::Boundary(reason) => JobsCommandError::Internal(reason),
                })?;
        let prepared_token_fd = prepared.token_fd;
        let refuse = |error: JobsCommandError| {
            close_fd(prepared_token_fd);
            error
        };

        // 2. Quota, against the submitter — the connection identity.
        let submitter_sid = peer.control.summary.caller_sid().to_string();
        let live = self.submitted.live_count_for_submitter(&submitter_sid);
        let limit = self.jobs_limits.max_jobs_per_submitter;
        if !is_quota_exempt(&submitter_sid) && live >= limit {
            return Err(refuse(JobsCommandError::QuotaExceeded {
                submitter_sid,
                live,
                limit,
            }));
        }

        // The descriptor: the submitter's, as given, or the default.
        let security_descriptor = match definition.security_descriptor_sddl.as_deref() {
            Some(sddl) => descriptor_factory.job_descriptor_from_sddl(sddl),
            None => descriptor_factory.default_job_descriptor(&submitter_sid),
        }
        .map_err(|error| {
            refuse(match error {
                JobDescriptorError::Invalid(reason) => JobsCommandError::InvalidDescriptor(reason),
                JobDescriptorError::Boundary(reason) => JobsCommandError::Internal(reason),
            })
        })?;

        // 3. The record, then the entry, then the launch queue — atomically.
        let mut work = SupervisorWork::from_supervisor(self);
        let job_id = work
            .job_ids
            .allocate_batch(1, now_ns)
            .map_err(SupervisorError::RequestIdAllocation)
            .map_err(|error| refuse(JobsCommandError::Internal(format!("{error:?}"))))?[0];
        let record = JobRecord::new_submitted(
            job_id,
            SubmittedJobSpec {
                identity_user_sid: prepared.user_sid.clone(),
                token_summary: prepared.summary.clone(),
                image_path: definition.image_path.clone(),
                arguments: definition.arguments.clone(),
                environment: definition.environment.clone(),
                working_directory: definition.working_directory.clone(),
                created_at_ns: now_ns,
            },
        );
        let cgroup_id = record.cgroup_id.clone();
        let job_event = work
            .jobs
            .create_job(record)
            .map_err(SupervisorError::JobStore)
            .map_err(|error| refuse(JobsCommandError::Internal(format!("{error:?}"))))?;

        let mut descriptors = descriptors;
        let output_sink_fd = if definition.output {
            descriptors.pop().map(IntoRawFd::into_raw_fd)
        } else {
            None
        };
        let attached_descriptors = definition
            .descriptor_names
            .iter()
            .cloned()
            .zip(descriptors.into_iter().map(IntoRawFd::into_raw_fd))
            .collect();
        let ready = (definition.readiness == JobReadiness::Notify).then_some(false);
        let entry = SubmittedJobEntry {
            job_id,
            submitter_sid: submitter_sid.clone(),
            identity: JobIdentity {
                user_sid: prepared.user_sid,
                logon_session: prepared.logon_session,
            },
            definition,
            security_descriptor,
            created_at_ns: now_ns,
            prepared_token_fd: Some(prepared_token_fd),
            attached_descriptors,
            output_sink_fd,
            ready,
            status_text: None,
            progress: None,
            progress_unit: None,
            last_status_event_ns: None,
            stopping_acknowledged: false,
            cause: None,
            stop: None,
            outcome: None,
            cgroup_id,
            cgroup_cleanup_due_at_ns: None,
            output_drop_reported: false,
        };
        work.submitted
            .insert(entry)
            .map_err(|error| JobsCommandError::Internal(format!("{error:?}")))?;
        work.pending_submitted_launches.push_back(job_id);
        work.commit(self);

        Ok((
            job_id,
            SupervisorJobSubmitDispatch {
                job_event,
                submitter_sid,
            },
        ))
    }
}

/// SYSTEM is exempt from the per-submitter quota (PSPU §7.A).
fn is_quota_exempt(submitter_sid: &str) -> bool {
    submitter_sid == "S-1-5-18"
}

pub(super) fn close_fd(fd: i32) {
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
    }
}
