use crate::execution::launch::LaunchCreatedJobRequest;
use crate::job::JobType;
use crate::service::ServiceDefinition;

use super::super::state::Supervisor;

impl Supervisor {
    pub(in crate::supervisor) fn launch_request(
        &self,
        job_id: crate::ids::JobId,
        launched_at_ns: u64,
    ) -> LaunchCreatedJobRequest {
        LaunchCreatedJobRequest {
            job_id,
            launched_at_ns,
            notify_socket_path: self.settings.notify_socket_path.clone(),
            setup_timeout_secs: self.launch_setup_timeout_secs(job_id),
            output_pipe_buffer_bytes: self.log_config.max_buffer_per_service_bytes,
        }
    }

    fn launch_setup_timeout_secs(&self, job_id: crate::ids::JobId) -> u64 {
        let Some(job) = self.jobs.get(job_id) else {
            return ServiceDefinition::DEFAULT_START_TIMEOUT_SECS;
        };
        let definition = job
            .service
            .as_deref()
            .and_then(|service| self.services.definition(service));
        match job.job_type {
            JobType::HealthCheck => definition
                .map(|definition| definition.health_check_timeout_secs)
                .unwrap_or(ServiceDefinition::DEFAULT_HEALTH_CHECK_TIMEOUT_SECS),
            JobType::ServiceMain
            | JobType::PreExecHook
            | JobType::PostExecHook
            | JobType::ReloadHook => definition
                .map(|definition| definition.start_timeout_secs)
                .unwrap_or(ServiceDefinition::DEFAULT_START_TIMEOUT_SECS),
            // Exec confirmation is bounded by the service default: a submitted
            // job's own timeouts start once the process is running.
            JobType::Submitted => ServiceDefinition::DEFAULT_START_TIMEOUT_SECS,
        }
    }
}
