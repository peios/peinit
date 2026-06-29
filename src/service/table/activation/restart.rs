use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use super::super::ServiceTable;
use super::super::model::{
    RestartBackoffDeadline, RestartWindowResetDeadline, ServiceTableError, ServiceTableTransition,
};

const NANOS_PER_SEC: u64 = 1_000_000_000;

impl ServiceTable {
    pub fn transition_service_to_restart_backoff(
        &mut self,
        service: &str,
        cause: TransitionCause,
        backoff_until_ns: u64,
    ) -> Result<ServiceTableTransition, ServiceTableError> {
        let transition = self.transition_service(
            service,
            ServiceTransition {
                to: ServiceState::Backoff,
                cause,
            },
        )?;
        let entry = self.require_entry_mut(service)?;
        entry.runtime.consecutive_restart_failures =
            entry.runtime.consecutive_restart_failures.saturating_add(1);
        entry.runtime.restart_backoff_until_ns = Some(backoff_until_ns);
        Ok(transition)
    }

    pub fn due_restart_backoffs(&self, now_ns: u64) -> Vec<RestartBackoffDeadline> {
        self.restart_backoff_deadlines()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .collect()
    }

    pub fn next_restart_backoff_deadline(&self) -> Option<RestartBackoffDeadline> {
        self.restart_backoff_deadlines()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
    }

    pub fn due_restart_window_resets(&self, now_ns: u64) -> Vec<RestartWindowResetDeadline> {
        self.restart_window_reset_deadlines()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .collect()
    }

    pub fn next_restart_window_reset_deadline_ns(&self) -> Option<u64> {
        self.restart_window_reset_deadlines()
            .map(|deadline| deadline.due_at_ns)
            .min()
    }

    pub fn reset_restart_failures_after_window(
        &mut self,
        service: &str,
    ) -> Result<(), ServiceTableError> {
        let entry = self.require_entry_mut(service)?;
        entry.runtime.consecutive_restart_failures = 0;
        Ok(())
    }

    fn restart_backoff_deadlines(&self) -> impl Iterator<Item = RestartBackoffDeadline> + '_ {
        self.entries
            .iter()
            .filter(|(_, entry)| !entry.definition_removed)
            .filter(|(_, entry)| entry.runtime.state == ServiceState::Backoff)
            .filter_map(|(service, entry)| {
                entry
                    .runtime
                    .restart_backoff_until_ns
                    .map(|due_at_ns| RestartBackoffDeadline {
                        service: service.clone(),
                        due_at_ns,
                    })
            })
    }

    fn restart_window_reset_deadlines(
        &self,
    ) -> impl Iterator<Item = RestartWindowResetDeadline> + '_ {
        self.entries
            .iter()
            .filter(|(_, entry)| !entry.definition_removed)
            .filter(|(_, entry)| entry.runtime.state == ServiceState::Active)
            .filter(|(_, entry)| entry.runtime.consecutive_restart_failures > 0)
            .filter_map(|(service, entry)| {
                entry.runtime.dependent_satisfied_since_ns.map(|since_ns| {
                    RestartWindowResetDeadline {
                        service: service.clone(),
                        due_at_ns: since_ns.saturating_add(
                            entry
                                .definition
                                .restart_window_secs
                                .saturating_mul(NANOS_PER_SEC),
                        ),
                    }
                })
            })
    }
}
