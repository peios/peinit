use crate::boundary::RegistryClient;
use crate::control::reload_config::{ReloadConfigError, ReloadConfigOutcome, reload_config};

use super::Supervisor;

impl Supervisor {
    pub(crate) fn reload_config_from_registry(
        &mut self,
        registry: &mut dyn RegistryClient,
    ) -> Result<ReloadConfigOutcome, ReloadConfigError> {
        // §3.7: a boot executes against its snapshot. Until the boot plan has
        // drained, nothing re-reads the registry — a boot-plan service that
        // has not started yet would otherwise start from a definition the
        // plan never saw (PEI-350). The request is not lost: the runtime runs
        // one coalesced reload once the plan drains.
        if self.refuse_reload_during_boot_window() {
            return Err(ReloadConfigError::BootInProgress);
        }
        let outcome = reload_config(registry, &mut self.services)?;
        self.control_security = outcome.control_security.clone();
        self.control_limits = outcome.control_limits;
        self.jobs_limits = outcome.jobs_limits;
        self.log_config = outcome.log_config.clone();
        self.settings.shutdown = outcome.shutdown_settings.clone();
        self.global_environment = outcome.global_environment.clone();
        self.eventd_log_socket_path = outcome.eventd_log_socket_path.clone();
        self.fd_store
            .retain_services(&self.services.service_names());
        Ok(outcome)
    }
}
