use crate::service::runtime::{
    ServiceRuntimeSnapshot, ServiceState, ServiceTransition, ServiceTransitionEvent,
    TransitionCause,
};

use super::super::ServiceTable;
use super::super::model::{
    ServiceActivationSnapshot, ServiceTableError, ServiceTableTransition,
    retains_definition_after_removal,
};

impl ServiceTable {
    pub fn prepare_activation_snapshot(
        &self,
        service: &str,
    ) -> Result<ServiceActivationSnapshot, ServiceTableError> {
        let entry = self.require_entry(service)?;
        if entry.definition_removed {
            return Err(ServiceTableError::DefinitionRemoved {
                service: service.to_string(),
            });
        }
        Ok(ServiceActivationSnapshot {
            service: service.to_string(),
            activation_generation: entry.runtime.generation + 1,
            cgroup_generation: entry.runtime.cgroup_generation,
            definition: entry.definition.clone(),
        })
    }

    pub fn transition_service(
        &mut self,
        service: &str,
        transition: ServiceTransition,
    ) -> Result<ServiceTableTransition, ServiceTableError> {
        let event = {
            let entry = self.require_entry_mut(service)?;
            let event = entry
                .runtime
                .transition(transition)
                .map_err(ServiceTableError::Transition)?;
            update_restart_metadata_after_transition(&mut entry.runtime, &event);
            event
        };
        self.apply_pending_definition_if_drained(service);
        let discarded_definition_removed = self.discard_drained_definition_removed(service);
        Ok(ServiceTableTransition {
            event,
            discarded_definition_removed,
        })
    }

    pub fn mark_dependent_satisfied_since(
        &mut self,
        service: &str,
        satisfied_at_ns: u64,
    ) -> Result<(), ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        entry
            .runtime
            .mark_dependent_satisfied_since(satisfied_at_ns);
        Ok(())
    }

    fn discard_drained_definition_removed(&mut self, service: &str) -> bool {
        let should_discard = self.entries.get(service).is_some_and(|entry| {
            entry.definition_removed && !retains_definition_after_removal(entry.runtime.state)
        });
        if should_discard {
            self.entries.remove(service);
        }
        should_discard
    }

    fn apply_pending_definition_if_drained(&mut self, service: &str) {
        let Some(entry) = self.entries.get_mut(service) else {
            return;
        };
        if entry.definition_removed || retains_definition_after_removal(entry.runtime.state) {
            return;
        }
        if let Some(pending_definition) = entry.pending_definition.take() {
            entry.definition = pending_definition;
        }
    }
}

fn update_restart_metadata_after_transition(
    runtime: &mut ServiceRuntimeSnapshot,
    event: &ServiceTransitionEvent,
) {
    if event.to != ServiceState::Backoff {
        runtime.restart_backoff_until_ns = None;
    }
    if event.to == ServiceState::Inactive
        && matches!(
            event.cause,
            TransitionCause::CleanExit | TransitionCause::ExplicitReset
        )
    {
        runtime.consecutive_restart_failures = 0;
    }
}
