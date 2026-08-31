mod atomicity;
mod success;

use crate::boundary::{BoundaryError, RegistryClient};
use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::registry::SUPPORTED_SERVICES_SCHEMA_VERSION;
use crate::service::{ServiceDefinition, ServiceEnvironmentVariable, ServiceTable};

#[derive(Debug, Clone)]
struct StaticRegistry {
    result: Result<Vec<ServiceDefinition>, BoundaryError>,
    schema_version: u32,
    control_security: ControlSecurityDescriptor,
    control_limits: ControlSocketLimits,
    max_log_line_length: Option<u32>,
    max_log_buffer_per_service: Option<u32>,
    log_read_bytes_per_event: Option<u32>,
    pre_eventd_buffer_bytes: Option<u32>,
    shutdown_timeout_secs: Option<u32>,
    global_environment: Vec<ServiceEnvironmentVariable>,
    eventd_log_socket_path: Option<String>,
    reads: usize,
}

impl StaticRegistry {
    fn services(services: Vec<ServiceDefinition>) -> Self {
        Self {
            result: Ok(services),
            schema_version: SUPPORTED_SERVICES_SCHEMA_VERSION,
            control_security: ControlSecurityDescriptor::Default,
            control_limits: ControlSocketLimits::default(),
            max_log_line_length: None,
            max_log_buffer_per_service: None,
            log_read_bytes_per_event: None,
            pre_eventd_buffer_bytes: None,
            shutdown_timeout_secs: None,
            global_environment: Vec::new(),
            eventd_log_socket_path: None,
            reads: 0,
        }
    }

    fn error(error: BoundaryError) -> Self {
        Self {
            result: Err(error),
            schema_version: SUPPORTED_SERVICES_SCHEMA_VERSION,
            control_security: ControlSecurityDescriptor::Default,
            control_limits: ControlSocketLimits::default(),
            max_log_line_length: None,
            max_log_buffer_per_service: None,
            log_read_bytes_per_event: None,
            pre_eventd_buffer_bytes: None,
            shutdown_timeout_secs: None,
            global_environment: Vec::new(),
            eventd_log_socket_path: None,
            reads: 0,
        }
    }

    fn schema_version(mut self, schema_version: u32) -> Self {
        self.schema_version = schema_version;
        self
    }

    fn control_security(mut self, control_security: ControlSecurityDescriptor) -> Self {
        self.control_security = control_security;
        self
    }

    fn control_limits(mut self, control_limits: ControlSocketLimits) -> Self {
        self.control_limits = control_limits;
        self
    }

    fn shutdown_timeout_secs(mut self, timeout_secs: u32) -> Self {
        self.shutdown_timeout_secs = Some(timeout_secs);
        self
    }

    fn log_config(mut self, max_line_length: u32, max_buffer_per_service: u32) -> Self {
        self.max_log_line_length = Some(max_line_length);
        self.max_log_buffer_per_service = Some(max_buffer_per_service);
        self
    }

    fn eventd_log_socket_path(mut self, path: impl Into<String>) -> Self {
        self.eventd_log_socket_path = Some(path.into());
        self
    }
}

impl StaticRegistry {
    fn with_max_log_line_length(mut self, value: Option<u32>) -> Self {
        self.max_log_line_length = value;
        self
    }

    fn with_max_log_buffer_per_service(mut self, value: Option<u32>) -> Self {
        self.max_log_buffer_per_service = value;
        self
    }

    fn with_log_read_bytes_per_event(mut self, value: Option<u32>) -> Self {
        self.log_read_bytes_per_event = value;
        self
    }

    fn with_pre_eventd_buffer_bytes(mut self, value: Option<u32>) -> Self {
        self.pre_eventd_buffer_bytes = value;
        self
    }
}

impl RegistryClient for StaticRegistry {
    fn read_service_definitions(&mut self) -> Result<Vec<ServiceDefinition>, BoundaryError> {
        self.reads += 1;
        self.result.clone()
    }

    fn read_services_schema_version(&mut self) -> Result<u32, BoundaryError> {
        Ok(self.schema_version)
    }

    fn read_control_security(&mut self) -> Result<ControlSecurityDescriptor, BoundaryError> {
        Ok(self.control_security.clone())
    }

    fn read_control_socket_limits(&mut self) -> Result<ControlSocketLimits, BoundaryError> {
        Ok(self.control_limits)
    }

    fn read_shutdown_timeout_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(self.shutdown_timeout_secs)
    }

    fn read_max_log_line_length(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(self.max_log_line_length)
    }

    fn read_max_log_buffer_per_service(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(self.max_log_buffer_per_service)
    }

    fn read_log_read_bytes_per_event(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(self.log_read_bytes_per_event)
    }

    fn read_pre_eventd_buffer_bytes(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(self.pre_eventd_buffer_bytes)
    }

    fn read_global_environment(
        &mut self,
    ) -> Result<Vec<ServiceEnvironmentVariable>, BoundaryError> {
        Ok(self.global_environment.clone())
    }

    fn read_eventd_log_socket_path(&mut self) -> Result<Option<String>, BoundaryError> {
        Ok(self.eventd_log_socket_path.clone())
    }
}

fn service(name: &str, image_path: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, image_path)
}

fn service_table(names: &[&str]) -> ServiceTable {
    ServiceTable::from_boot_snapshot(
        names
            .iter()
            .map(|name| service(name, &format!("/sbin/{name}")))
            .collect(),
    )
    .expect("service table")
}
