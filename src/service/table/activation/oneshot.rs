use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};
use crate::service::{ServiceTable, ServiceTableError, ServiceTableTransition, ServiceType};

impl ServiceTable {
    pub fn release_completed_oneshot_if_not_retained(
        &mut self,
        service: &str,
    ) -> Result<Option<ServiceTableTransition>, ServiceTableError> {
        let should_release = {
            let entry = self.require_entry(service)?;
            entry.definition.service_type == ServiceType::Oneshot
                && !entry.definition.remain_after_exit
                && entry.runtime.state == ServiceState::Completed
        };

        if !should_release {
            return Ok(None);
        }

        self.transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Inactive,
                cause: TransitionCause::CleanExit,
            },
        )
        .map(Some)
    }
}
