use crate::boundary::BoundaryError;
use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::jobs::socket::JobsSocketLimits;
use crate::logging::RuntimeLogConfig;
use crate::registry::RegistryConfigWarning;
use crate::service::{
    ServiceEnvironmentVariable, ServiceGraphValidationFailure, ServiceGraphWarning,
    ServiceReloadSummary, ServiceTableError,
};
use crate::shutdown::ShutdownSettings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadConfigOutcome {
    pub summary: ServiceReloadSummary,
    pub services_schema_version: u32,
    pub config_warnings: Vec<RegistryConfigWarning>,
    pub control_security: ControlSecurityDescriptor,
    pub control_limits: ControlSocketLimits,
    pub jobs_limits: JobsSocketLimits,
    pub log_config: RuntimeLogConfig,
    pub shutdown_settings: ShutdownSettings,
    pub global_environment: Vec<ServiceEnvironmentVariable>,
    pub eventd_log_socket_path: Option<String>,
    pub warnings: Vec<ServiceGraphWarning>,
}

impl ReloadConfigOutcome {
    pub fn warning_messages(&self) -> Vec<String> {
        self.config_warnings
            .iter()
            .map(ToString::to_string)
            .chain(self.warnings.iter().map(service_graph_warning_message))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadConfigError {
    Registry(BoundaryError),
    Validation(ServiceGraphValidationFailure),
    ServiceTable(ServiceTableError),
}

fn service_graph_warning_message(warning: &ServiceGraphWarning) -> String {
    match warning {
        ServiceGraphWarning::AliveReadinessWithHardDependents {
            service,
            dependents,
        } => format!(
            "service {service} uses Alive readiness while hard dependents require readiness: {}",
            dependents.join(", "),
        ),
        ServiceGraphWarning::UnfilledRole { role, services } => format!(
            "no service provides {role}, which these services need to start: {}",
            services.join(", "),
        ),
    }
}
