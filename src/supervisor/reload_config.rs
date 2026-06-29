use crate::boundary::RegistryClient;
use crate::control::reload_config::{ReloadConfigError, ReloadConfigOutcome, reload_config};

use super::Supervisor;

impl Supervisor {
    pub(crate) fn reload_config_from_registry(
        &mut self,
        registry: &mut dyn RegistryClient,
    ) -> Result<ReloadConfigOutcome, ReloadConfigError> {
        let outcome = reload_config(registry, &mut self.services)?;
        self.control_security = outcome.control_security.clone();
        self.control_limits = outcome.control_limits;
        self.log_config = outcome.log_config.clone();
        self.settings.shutdown = outcome.shutdown_settings.clone();
        self.global_environment = outcome.global_environment.clone();
        self.eventd_log_socket_path = outcome.eventd_log_socket_path.clone();
        self.fd_store
            .retain_services(&self.services.service_names());
        Ok(outcome)
    }
}
