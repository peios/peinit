use crate::boundary::{BoundaryError, LaunchedProcess, ProcessLaunchSpec, ProcessLauncher};
use crate::job::{JobRecord, JobState, JobType};

use super::model::LaunchCreatedJobError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LaunchTarget {
    ServiceMain,
    ReloadHook,
    PreExecHook,
    PostExecHook,
    HealthCheck,
}

impl LaunchTarget {
    pub(super) fn validate(
        self,
        job: &JobRecord,
        launched_at_ns: u64,
    ) -> Result<(), LaunchCreatedJobError> {
        self.validate_job_type(job)?;
        validate_created_at(job, launched_at_ns)?;
        Ok(())
    }

    pub(super) fn launch(
        self,
        launcher: &mut (impl ProcessLauncher + ?Sized),
        spec: ProcessLaunchSpec<'_>,
    ) -> Result<LaunchedProcess, BoundaryError> {
        match self {
            Self::ServiceMain => launcher.launch_service(spec),
            Self::ReloadHook | Self::PreExecHook | Self::PostExecHook | Self::HealthCheck => {
                launcher.launch_job(spec)
            }
        }
    }

    fn validate_job_type(self, job: &JobRecord) -> Result<(), LaunchCreatedJobError> {
        match self {
            Self::ServiceMain => validate_service_main_job(job),
            Self::ReloadHook => validate_reload_hook_job(job),
            Self::PreExecHook => validate_pre_exec_hook_job(job),
            Self::PostExecHook => validate_post_exec_hook_job(job),
            Self::HealthCheck => validate_health_check_job(job),
        }
    }
}

fn validate_service_main_job(job: &JobRecord) -> Result<(), LaunchCreatedJobError> {
    if job.job_type != JobType::ServiceMain {
        return Err(LaunchCreatedJobError::NotServiceMainJob { job_id: job.id });
    }
    if job.service.is_none() {
        return Err(LaunchCreatedJobError::ServiceMainJobMissingService { job_id: job.id });
    }
    Ok(())
}

fn validate_reload_hook_job(job: &JobRecord) -> Result<(), LaunchCreatedJobError> {
    if job.job_type == JobType::ReloadHook {
        Ok(())
    } else {
        Err(LaunchCreatedJobError::NotReloadHookJob {
            job_id: job.id,
            job_type: job.job_type,
        })
    }
}

fn validate_pre_exec_hook_job(job: &JobRecord) -> Result<(), LaunchCreatedJobError> {
    if job.job_type == JobType::PreExecHook {
        Ok(())
    } else {
        Err(LaunchCreatedJobError::NotPreExecHookJob {
            job_id: job.id,
            job_type: job.job_type,
        })
    }
}

fn validate_post_exec_hook_job(job: &JobRecord) -> Result<(), LaunchCreatedJobError> {
    if job.job_type == JobType::PostExecHook {
        Ok(())
    } else {
        Err(LaunchCreatedJobError::NotPostExecHookJob {
            job_id: job.id,
            job_type: job.job_type,
        })
    }
}

fn validate_health_check_job(job: &JobRecord) -> Result<(), LaunchCreatedJobError> {
    if job.job_type == JobType::HealthCheck {
        Ok(())
    } else {
        Err(LaunchCreatedJobError::NotHealthCheckJob {
            job_id: job.id,
            job_type: job.job_type,
        })
    }
}

fn validate_created_at(job: &JobRecord, launched_at_ns: u64) -> Result<(), LaunchCreatedJobError> {
    if job.state != JobState::Created {
        return Err(LaunchCreatedJobError::JobNotCreated {
            job_id: job.id,
            state: job.state,
        });
    }
    if launched_at_ns < job.created_at_ns {
        return Err(LaunchCreatedJobError::StartBeforeCreation {
            job_id: job.id,
            created_at_ns: job.created_at_ns,
            launched_at_ns,
        });
    }
    Ok(())
}
