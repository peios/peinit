use crate::boot::phase2::{Phase2BootPlan, SafeModeDowngrade};
use crate::boot::{BootMode, BootModeReason};
use crate::boundary::BootAttemptCounter;
use crate::control::query::BootStatusView;
use crate::service::{ErrorControl, ServiceTable};

use super::dispatch::SupervisorBootSuccessDispatch;

const NANOS_PER_SEC: u64 = 1_000_000_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BootSuccessTracker {
    required_critical_services: Vec<String>,
    grace_secs: u32,
    empty_critical_satisfied_since_ns: Option<u64>,
    reset_attempted: bool,
    /// Why the reset failed, when it did. Kept for `boot` to report: a boot
    /// whose counter was not reset will count as a failed attempt at the
    /// next one, whatever its services did.
    reset_error: Option<String>,
}

/// The mode Phase 2 booted in, and every finding that forced it down to
/// Safe if one did. Taken from the plan, which is the only place the
/// downgrade findings survive (TRM §2.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootModeRecord {
    pub mode: BootMode,
    pub downgrade: Vec<SafeModeDowngrade>,
}

impl BootModeRecord {
    pub fn from_plan(plan: &Phase2BootPlan) -> Self {
        Self {
            mode: plan.mode,
            downgrade: plan.safe_mode_downgrade.clone(),
        }
    }

    /// The planner looks for downgrade findings only in a Full boot, so a
    /// boot with findings was not asked for in Safe; one in Safe or
    /// Recovery without them was asked for.
    pub fn reason(&self) -> BootModeReason {
        if !self.downgrade.is_empty() {
            BootModeReason::SafeModeDowngrade
        } else if self.mode == BootMode::Full {
            BootModeReason::Normal
        } else {
            BootModeReason::Requested
        }
    }
}

/// Where the boot stands against its success criterion (TRM §2.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootConfirmation {
    pub confirmed: bool,
    pub waiting_on: Vec<String>,
    pub due_at_ns: Option<u64>,
    pub error: Option<String>,
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
        self.reset_error = None;
    }

    /// Whether the boot has counted, and if not, what it is waiting for.
    pub fn confirmation(&self, services: &ServiceTable) -> BootConfirmation {
        if self.reset_attempted {
            return BootConfirmation {
                confirmed: self.reset_error.is_none(),
                waiting_on: Vec::new(),
                due_at_ns: None,
                error: self.reset_error.clone(),
            };
        }
        BootConfirmation {
            confirmed: false,
            waiting_on: self
                .required_critical_services
                .iter()
                .filter(|service| {
                    services.runtime(service).is_none_or(|runtime| {
                        !runtime.state.satisfies_dependents()
                            || runtime.dependent_satisfied_since_ns.is_none()
                    })
                })
                .cloned()
                .collect(),
            due_at_ns: self
                .next_deadline(services)
                .map(|deadline| deadline.due_at_ns),
            error: None,
        }
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
        self.reset_error = reset_result
            .as_ref()
            .err()
            .map(|error| format!("{error:?}"));
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

impl super::Supervisor {
    /// How this boot went: what `boot` answers (PSPU §4.15).
    pub fn boot_status(&self) -> BootStatusView {
        let record = self.boot_mode.clone().unwrap_or(BootModeRecord {
            mode: self.settings.phase2.mode,
            downgrade: Vec::new(),
        });
        let confirmation = self.boot_success.confirmation(&self.services);
        BootStatusView {
            mode: record.mode,
            reason: record.reason(),
            downgrade: record.downgrade.iter().map(ToString::to_string).collect(),
            attempts: self.settings.boot_attempts.counted,
            max_attempts: self.settings.boot_attempts.threshold,
            confirmed: confirmation.confirmed,
            grace_seconds: self.settings.phase2.boot_success_grace_secs,
            waiting_on: confirmation.waiting_on,
            confirms_at_ns: confirmation.due_at_ns,
            confirm_error: confirmation.error,
        }
    }
}

fn is_critical_service(services: &ServiceTable, service: &str) -> bool {
    services
        .definition(service)
        .is_some_and(|definition| definition.error_control == ErrorControl::Critical)
}
