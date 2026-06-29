use crate::boundary::ProcessController;
use crate::job::{JobState, JobStore};
use crate::service::ServiceTable;

use super::model::{AuthenticatedNotifySender, NotifyApplyError};

pub fn authenticate_notify_sender<P>(
    services: &ServiceTable,
    jobs: &JobStore,
    controller: &mut P,
    sender_pid: u32,
) -> Result<AuthenticatedNotifySender, NotifyApplyError>
where
    P: ProcessController + ?Sized,
{
    for service in services.service_names() {
        let Some(job_id) = jobs.current_service_main_job(service) else {
            continue;
        };
        let job = jobs
            .get(job_id)
            .ok_or(NotifyApplyError::MissingService { job_id })?;
        if job.pid != Some(sender_pid) {
            continue;
        }
        if job.state != JobState::Running {
            return Err(NotifyApplyError::JobNotRunning {
                job_id,
                state: job.state,
            });
        }
        let Some(pidfd) = job.pidfd else {
            return Err(NotifyApplyError::MissingProcess { job_id });
        };
        if !controller
            .pidfd_matches_pid(pidfd, sender_pid)
            .map_err(|error| NotifyApplyError::ProcessVerification {
                job_id,
                pid: sender_pid,
                pidfd,
                message: format!("{error:?}"),
            })?
        {
            return Err(NotifyApplyError::PidfdMismatch {
                job_id,
                pid: sender_pid,
                pidfd,
            });
        }
        let runtime = services.runtime(service).ok_or_else(|| {
            NotifyApplyError::ServiceTable(crate::service::ServiceTableError::UnknownService {
                service: service.to_string(),
            })
        })?;
        if job.activation_generation != runtime.generation {
            return Err(NotifyApplyError::GenerationMismatch {
                service: service.to_string(),
                job_generation: job.activation_generation,
                runtime_generation: runtime.generation,
            });
        }
        return Ok(AuthenticatedNotifySender {
            service: service.to_string(),
            job_id,
            operation_id: job.operation_id,
            generation: job.activation_generation,
            job_created_at_ns: job.created_at_ns,
            cgroup_generation: job.cgroup_generation,
        });
    }

    Err(NotifyApplyError::UnauthenticatedSender { pid: sender_pid })
}
