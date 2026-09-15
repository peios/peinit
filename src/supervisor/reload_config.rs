use crate::boundary::RegistryClient;
use crate::control::reload_config::{
    ReloadConfigError, ReloadConfigOutcome, reload_config_with_frozen,
};

use super::Supervisor;

impl Supervisor {
    pub(crate) fn reload_config_from_registry(
        &mut self,
        registry: &mut dyn RegistryClient,
    ) -> Result<ReloadConfigOutcome, ReloadConfigError> {
        // §3.7: a boot executes against its snapshot. The reload runs, but a
        // boot-plan member whose launch has not been attempted keeps the
        // plan's definition and takes the new one as pending; the reload
        // after the window applies it (PEI-350).
        let frozen = self.frozen_boot_plan_members();
        let outcome = reload_config_with_frozen(registry, &mut self.services, &frozen)?;
        self.record_deferred_definitions(&outcome.summary.deferred);
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
