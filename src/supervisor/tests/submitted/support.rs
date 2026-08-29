//! Fakes for the two boundaries a submitted job crosses that a service
//! never does: establishing the job's identity and minting/evaluating the
//! per-job descriptor. Every descriptor these hand out is real (a dup of
//! `/dev/null`) because the supervisor closes them as it would on Peios.

use std::fs::File;
use std::os::fd::{IntoRawFd, OwnedFd};

use crate::boundary::{
    BoundaryError, ChildExitStatus, ChildReap, JobIdentityError, JobIdentityProvider,
    JobIdentitySource, PreparedJobIdentity, ShutdownFinalizer, TokenHandle, TokenProvider,
};
use crate::control::system::ControlPeer;
use crate::ids::JobId;
use crate::job::JobRecord;
use crate::jobs::connection::JobsPeer;
use crate::jobs::socket::{JobsMessage, JobsSocketLimits};
use crate::security::TokenSummary;
use crate::submitted::{
    JobAccess, JobAccessCheckError, JobAccessCheckRequest, JobAccessChecker, JobAccessDecision,
    JobDescriptorError, JobDescriptorFactory, JobSecurityDescriptor,
};
use crate::supervisor::{
    Supervisor, SupervisorJobsCommandDispatch, SupervisorJobsMessageContext,
    SupervisorJobsMessageResponse, SupervisorSettings, SupervisorSubmittedLaunchResult,
};

use super::super::{
    BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController, TestProcessLauncher, process,
    settings,
};

pub(super) const SUBMIT_NS: u64 = 2_000_000_000;
pub(super) const LAUNCH_NS: u64 = 2_000_010_000;
pub(super) const IDENTITY_SID: &str = "S-1-5-21-1-2-3-1001";
pub(super) const IDENTITY_LOGON_SESSION: u64 = 0x3e7;
pub(super) const SUBMITTER: &str = "backupd";
pub(super) const SYSTEM_SID: &str = "S-1-5-18";

pub(super) fn dev_null_fd() -> OwnedFd {
    File::open("/dev/null").expect("open /dev/null").into()
}

/// A pidfd the answer paths can duplicate: any real descriptor will do for
/// a controller that never inspects it. Owned by the test process for its
/// lifetime, as a supervised job's handle would be.
pub(super) fn real_pidfd() -> i32 {
    dev_null_fd().into_raw_fd()
}

/// The identity boundary, scripted: each preparation hands back a fresh
/// descriptor as the "primary token" and records which route was used.
#[derive(Debug, Default)]
pub(super) struct TestJobIdentityProvider {
    pub(super) attached_token_sources: usize,
    pub(super) peer_primary_sources: usize,
    pub(super) refuse_with: Option<JobIdentityError>,
}

impl JobIdentityProvider for TestJobIdentityProvider {
    fn prepare_job_identity(
        &mut self,
        source: JobIdentitySource,
    ) -> Result<PreparedJobIdentity, JobIdentityError> {
        match source {
            JobIdentitySource::AttachedToken { token } => {
                drop(token);
                self.attached_token_sources += 1;
            }
            JobIdentitySource::PeerPrimary { .. } => self.peer_primary_sources += 1,
        }
        if let Some(error) = self.refuse_with.clone() {
            return Err(error);
        }
        Ok(PreparedJobIdentity {
            token_fd: dev_null_fd().into_raw_fd(),
            user_sid: IDENTITY_SID.to_string(),
            logon_session: IDENTITY_LOGON_SESSION,
            summary: TokenSummary::requested_identity("user"),
        })
    }
}

/// The security boundary, scripted: descriptors are the submitter SID in
/// bytes (so a test can see whose descriptor a job got) and access checks
/// answer from a per-right allow list, recording every question asked.
#[derive(Debug)]
pub(super) struct TestJobSecurity {
    pub(super) allowed: JobAccess,
    pub(super) checks: Vec<(JobAccess, Vec<u8>)>,
    pub(super) sddl_error: Option<String>,
}

impl Default for TestJobSecurity {
    fn default() -> Self {
        Self {
            allowed: JobAccess::ALL,
            checks: Vec::new(),
            sddl_error: None,
        }
    }
}

impl TestJobSecurity {
    pub(super) fn allowing(allowed: JobAccess) -> Self {
        Self {
            allowed,
            ..Self::default()
        }
    }
}

impl JobDescriptorFactory for TestJobSecurity {
    fn default_job_descriptor(
        &mut self,
        submitter_sid: &str,
    ) -> Result<JobSecurityDescriptor, JobDescriptorError> {
        Ok(JobSecurityDescriptor {
            bytes: submitter_sid.as_bytes().to_vec(),
        })
    }

    fn job_descriptor_from_sddl(
        &mut self,
        sddl: &str,
    ) -> Result<JobSecurityDescriptor, JobDescriptorError> {
        match &self.sddl_error {
            Some(reason) => Err(JobDescriptorError::Invalid(reason.clone())),
            None => Ok(JobSecurityDescriptor {
                bytes: format!("sddl:{sddl}").into_bytes(),
            }),
        }
    }
}

impl JobAccessChecker for TestJobSecurity {
    fn check_job_access(
        &mut self,
        request: JobAccessCheckRequest<'_>,
    ) -> Result<JobAccessDecision, JobAccessCheckError> {
        self.checks
            .push((request.desired_access, request.descriptor.bytes.clone()));
        let allowed =
            self.allowed.bits() & request.desired_access.bits() == request.desired_access.bits();
        Ok(JobAccessDecision {
            allowed,
            granted_access_bits: if allowed {
                request.desired_access.bits()
            } else {
                0
            },
        })
    }
}

/// A token provider that also materialises prepared tokens, as the Linux
/// one does; it records the prepared descriptors it was handed.
#[derive(Debug, Default)]
pub(super) struct SubmittedTokenProvider {
    pub(super) prepared_token_fds: Vec<i32>,
    pub(super) fail_materialisation: bool,
}

impl TokenProvider for SubmittedTokenProvider {
    fn materialize_service_token(&mut self, job: &JobRecord) -> Result<TokenHandle, BoundaryError> {
        Ok(TokenHandle {
            fd: 8,
            identity: job.resolved_identity.clone(),
            summary: TokenSummary::requested_identity(job.resolved_identity.clone()),
        })
    }

    fn materialize_prepared_token(
        &mut self,
        job: &JobRecord,
        prepared_token_fd: i32,
    ) -> Result<TokenHandle, BoundaryError> {
        self.prepared_token_fds.push(prepared_token_fd);
        if self.fail_materialisation {
            return Err(BoundaryError::Token("scripted token failure".to_string()));
        }
        Ok(TokenHandle {
            fd: dev_null_fd().into_raw_fd(),
            identity: job.resolved_identity.clone(),
            summary: job.token_summary.clone(),
        })
    }
}

#[derive(Debug, Default)]
pub(super) struct NoopFinalizer;

impl ShutdownFinalizer for NoopFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        Ok(Vec::new())
    }

    fn unmount(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn remount_readonly(&mut self, _mount_point: &str) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn reboot(&mut self, _kind: crate::shutdown::ShutdownKind) -> Result<(), BoundaryError> {
        Ok(())
    }
}

pub(super) fn jobs_peer(identity: &str) -> JobsPeer {
    JobsPeer {
        control: ControlPeer::borrowed_token_fd(44, TokenSummary::requested_identity(identity)),
        pidfd: dev_null_fd(),
    }
}

pub(super) fn message(payload: &str) -> JobsMessage {
    JobsMessage {
        payload: payload.as_bytes().to_vec(),
        token: None,
        descriptors: Vec::new(),
        truncated: false,
        control_truncated: false,
    }
}

pub(super) fn message_with(payload: &str, token: bool, descriptors: usize) -> JobsMessage {
    JobsMessage {
        payload: payload.as_bytes().to_vec(),
        token: token.then(dev_null_fd),
        descriptors: (0..descriptors).map(|_| dev_null_fd()).collect(),
        truncated: false,
        control_truncated: false,
    }
}

pub(super) fn submit_payload(extra: &str) -> String {
    let comma = if extra.is_empty() { "" } else { "," };
    format!(r#"{{"command":"submit","image_path":"/usr/bin/backup"{comma}{extra}}}"#)
}

/// A booted supervisor with no services: the jobs system alone.
pub(super) fn submitted_supervisor() -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(Vec::new());
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
}

pub(super) fn set_jobs_limits(supervisor: &mut Supervisor, limits: JobsSocketLimits) {
    supervisor.jobs_limits = limits;
}

pub(super) struct Boundaries {
    pub(super) identity: TestJobIdentityProvider,
    pub(super) security: TestJobSecurity,
    pub(super) controller: TestProcessController,
    pub(super) clock: ScriptedClock,
}

impl Boundaries {
    pub(super) fn at(times: impl Into<std::collections::VecDeque<u64>>) -> Self {
        Self {
            identity: TestJobIdentityProvider::default(),
            security: TestJobSecurity::default(),
            controller: TestProcessController::default(),
            clock: ScriptedClock::new(times),
        }
    }

    pub(super) fn run(
        &mut self,
        supervisor: &mut Supervisor,
        peer: &JobsPeer,
        message: JobsMessage,
    ) -> SupervisorJobsMessageResponse {
        supervisor
            .run_jobs_message(
                peer,
                message,
                SupervisorJobsMessageContext {
                    identity_provider: &mut self.identity,
                    security: &mut self.security,
                    controller: &mut self.controller,
                    clock: &mut self.clock,
                },
            )
            .expect("serialise jobs response")
    }
}

/// Submit with `payload` and return the job id the submit dispatch names.
pub(super) fn submit(
    supervisor: &mut Supervisor,
    boundaries: &mut Boundaries,
    peer: &JobsPeer,
    message: JobsMessage,
) -> JobId {
    let response = boundaries.run(supervisor, peer, message);
    assert_eq!(response.error, None, "submit refused");
    match response.dispatch {
        Some(SupervisorJobsCommandDispatch::Submit(dispatch)) => dispatch.job_event.job_id,
        other => panic!("expected submit dispatch, got {other:?}"),
    }
}

/// Launch the queued job as `pid`/`pidfd` and return the launch dispatch.
pub(super) fn launch(
    supervisor: &mut Supervisor,
    controller: &mut TestProcessController,
    pid: u32,
    pidfd: i32,
) -> (
    SupervisorSubmittedLaunchResult,
    SubmittedTokenProvider,
    TestProcessLauncher,
) {
    let mut tokens = SubmittedTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(pid, pidfd)]);
    let mut clock = ScriptedClock::new([LAUNCH_NS]);
    let result = supervisor
        .launch_next_pending_submitted_job(&mut tokens, &mut launcher, &mut clock, controller)
        .expect("launch")
        .expect("a queued launch");
    (result, tokens, launcher)
}

/// Submit a plain job and launch it: the running job most tests start from.
pub(super) fn running_job(
    supervisor: &mut Supervisor,
    boundaries: &mut Boundaries,
    pid: u32,
    pidfd: i32,
) -> JobId {
    let peer = jobs_peer(SUBMITTER);
    let job_id = submit(supervisor, boundaries, &peer, message(&submit_payload("")));
    let (result, _, _) = launch(supervisor, &mut boundaries.controller, pid, pidfd);
    assert!(
        matches!(result, SupervisorSubmittedLaunchResult::Launched(_)),
        "expected a launched job, got {result:?}"
    );
    job_id
}

pub(super) fn reap(
    supervisor: &mut Supervisor,
    controller: &mut TestProcessController,
    pid: u32,
    status: ChildExitStatus,
    ended_at_ns: u64,
) -> crate::supervisor::SupervisorChildReapTurn {
    supervisor
        .apply_reaped_child(
            ChildReap { pid, status },
            ended_at_ns,
            controller,
            &mut NoopFinalizer,
        )
        .expect("reap")
}

pub(super) fn response_json(frame: &[u8]) -> serde_json::Value {
    serde_json::from_slice(frame).expect("response json")
}
