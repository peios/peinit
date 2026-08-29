use crate::boundary::{
    BoundaryError, RegistryClient, ServiceDefinitionsRead, TimerLastRunWriteOutcome,
    TimerLastRunWriteRequest, TimerLastRunWriter,
};
use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::provisioning::ProvisionedPathRegistrySnapshot;
use crate::service::{ServiceDefinition, ServiceEnvironmentVariable};
use crate::timer::state::TimerLastRunStorage;

use super::boot::{
    read_lcs_boot_success_grace_secs, read_lcs_max_parallel_starts,
    read_lcs_post_kill_timeout_secs, read_lcs_settle_timeout_secs, read_lcs_shutdown_timeout_secs,
};
use super::eventd::read_lcs_eventd_log_socket_path;
use super::global_env::read_lcs_global_environment;
use super::init::{
    read_lcs_control_security, read_lcs_control_socket_limits, read_lcs_log_read_bytes_per_event,
    read_lcs_max_log_buffer_per_service, read_lcs_max_log_line_length,
    read_lcs_pre_eventd_buffer_bytes,
};
use super::provisioning::read_lcs_provisioned_paths;
use super::schema::{provision_lcs_base_registry, read_lcs_services_schema_version};
use super::service::read_lcs_service_definitions;
use super::timer;

#[derive(Debug, Default, Clone, Copy)]
pub struct LcsRegistryClient;

#[derive(Debug, Default, Clone, Copy)]
pub struct LcsTimerLastRunWriter;

impl RegistryClient for LcsRegistryClient {
    fn read_service_definitions(&mut self) -> Result<Vec<ServiceDefinition>, BoundaryError> {
        let read = self.read_service_definitions_partial()?;
        // The strict view, for reload-config: any undecodable key fails the
        // whole read, so the transaction aborts and the running configuration
        // stands.
        if let Some(first) = read.undecodable.first() {
            return Err(BoundaryError::Registry(format!(
                "service {} failed to decode: {}",
                first.name, first.message
            )));
        }
        Ok(read.definitions)
    }

    fn read_service_definitions_partial(
        &mut self,
    ) -> Result<ServiceDefinitionsRead, BoundaryError> {
        read_lcs_service_definitions()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn provision_base_registry(&mut self) -> Result<(), BoundaryError> {
        provision_lcs_base_registry().map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_global_environment(
        &mut self,
    ) -> Result<Vec<ServiceEnvironmentVariable>, BoundaryError> {
        read_lcs_global_environment().map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_provisioned_paths(&mut self) -> Result<ProvisionedPathRegistrySnapshot, BoundaryError> {
        read_lcs_provisioned_paths().map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_max_parallel_starts(&mut self) -> Result<Option<u32>, BoundaryError> {
        read_lcs_max_parallel_starts()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_boot_success_grace_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        read_lcs_boot_success_grace_secs()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_shutdown_timeout_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        read_lcs_shutdown_timeout_secs()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_post_kill_timeout_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        read_lcs_post_kill_timeout_secs()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_settle_timeout_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        read_lcs_settle_timeout_secs()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_log_read_bytes_per_event(&mut self) -> Result<Option<u32>, BoundaryError> {
        read_lcs_log_read_bytes_per_event()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_pre_eventd_buffer_bytes(&mut self) -> Result<Option<u32>, BoundaryError> {
        read_lcs_pre_eventd_buffer_bytes()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_max_log_line_length(&mut self) -> Result<Option<u32>, BoundaryError> {
        read_lcs_max_log_line_length()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_max_log_buffer_per_service(&mut self) -> Result<Option<u32>, BoundaryError> {
        read_lcs_max_log_buffer_per_service()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_eventd_log_socket_path(&mut self) -> Result<Option<String>, BoundaryError> {
        read_lcs_eventd_log_socket_path()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_services_schema_version(&mut self) -> Result<u32, BoundaryError> {
        read_lcs_services_schema_version()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_control_security(&mut self) -> Result<ControlSecurityDescriptor, BoundaryError> {
        read_lcs_control_security().map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_control_socket_limits(&mut self) -> Result<ControlSocketLimits, BoundaryError> {
        read_lcs_control_socket_limits()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_jobs_socket_limits(
        &mut self,
    ) -> Result<crate::jobs::socket::JobsSocketLimits, BoundaryError> {
        super::init::read_lcs_jobs_socket_limits()
            .map_err(|error| BoundaryError::Registry(format!("{error:?}")))
    }

    fn read_timer_last_run(
        &mut self,
        service: &str,
        schedule: &str,
        storage: TimerLastRunStorage,
    ) -> Result<Option<u64>, BoundaryError> {
        timer::read_timer_last_run(service, schedule, storage)
            .map_err(|error| BoundaryError::Registry(error.to_string()))
    }
}

impl TimerLastRunWriter for LcsTimerLastRunWriter {
    fn queue_timer_last_run_write(
        &mut self,
        request: TimerLastRunWriteRequest,
    ) -> Result<TimerLastRunWriteOutcome, BoundaryError> {
        timer::queue_timer_last_run_write(
            request.service,
            request.schedule,
            request.storage,
            request.timestamp_realtime_ns,
        )
        .map(|()| TimerLastRunWriteOutcome::Queued)
        .map_err(|error| BoundaryError::Registry(error.to_string()))
    }
}
