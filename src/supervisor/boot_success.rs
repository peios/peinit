use crate::boot::BootMode;
use crate::boot::phase2::Phase2BootPlan;
use crate::boundary::BootAttemptCounter;
use crate::service::{ErrorControl, ServiceTable};

use super::dispatch::SupervisorBootSuccessDispatch;

const NANOS_PER_SEC: u64 = 1_000_000_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BootSuccessTracker {
    required_critical_services: Vec<String>,
    grace_secs: u32,
    empty_critical_satisfied_since_ns: Option<u64>,
    reset_attempted: bool,
}

impl BootSuccessTracker {
    pub fn configure_phase2(
        &mut self,
        services: &ServiceTable,
        plan: &Phase2BootPlan,
        retained_satisfied: &[String],
        grace_secs: u32,
    ) {
        let mut candidates = plan
            .starts
            .iter()
            .map(|start| start.service.as_str())
            .chain(plan.blocked.iter().map(|blocked| blocked.service.as_str()))
            .chain(retained_satisfied.iter().map(String::as_str))
            .filter(|service| is_critical_service(services, service))
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();

        self.required_critical_services = candidates;
        self.grace_secs = grace_secs;
        self.empty_critical_satisfied_since_ns = if plan.mode == BootMode::Recovery {
            None
        } else {
            Some(plan.observed_at_ns)
        };
        self.reset_attempted = false;
    }

    pub fn next_deadline(&self, services: &ServiceTable) -> Option<BootSuccessDeadline> {
        if self.reset_attempted {
            return None;
        }
        let satisfied_since_ns = self.required_satisfied_since_ns(services)?;
        Some(BootSuccessDeadline {
            due_at_ns: satisfied_since_ns
                .saturating_add(u64::from(self.grace_secs).saturating_mul(NANOS_PER_SEC)),
            satisfied_since_ns,
            required_critical_services: self.required_critical_services.clone(),
        })
    }

    pub fn process_due<C>(
        &mut self,
        services: &ServiceTable,
        counter: &mut C,
        now_ns: u64,
    ) -> Option<SupervisorBootSuccessDispatch>
    where
        C: BootAttemptCounter + ?Sized,
    {
        let deadline = self.next_deadline(services)?;
        if deadline.due_at_ns > now_ns {
            return None;
        }

        self.reset_attempted = true;
        let reset_result = counter.reset_boot_attempt_counter();
        Some(SupervisorBootSuccessDispatch {
            required_critical_services: deadline.required_critical_services,
            satisfied_since_ns: deadline.satisfied_since_ns,
            due_at_ns: deadline.due_at_ns,
            reset_result,
        })
    }

    fn required_satisfied_since_ns(&self, services: &ServiceTable) -> Option<u64> {
        if self.required_critical_services.is_empty() {
            return self.empty_critical_satisfied_since_ns;
        }

        let mut satisfied_since_ns = 0;
        for service in &self.required_critical_services {
            let runtime = services.runtime(service)?;
            if !runtime.state.satisfies_dependents() {
                return None;
            }
            let service_satisfied_since_ns = runtime.dependent_satisfied_since_ns?;
            satisfied_since_ns = satisfied_since_ns.max(service_satisfied_since_ns);
        }
        Some(satisfied_since_ns)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootSuccessDeadline {
    pub due_at_ns: u64,
    pub satisfied_since_ns: u64,
    pub required_critical_services: Vec<String>,
}

fn is_critical_service(services: &ServiceTable, service: &str) -> bool {
    services
        .definition(service)
        .is_some_and(|definition| definition.error_control == ErrorControl::Critical)
}
