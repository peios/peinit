//! The immediate reboot owed to a Critical service that has run out of
//! restart budget.
//!
//! `ErrorControl=Critical` means the machine is configured to reboot rather
//! than run without this service, and the reboot takes precedence over
//! `OnFailure` — peinit must not start the handler as well.
//!
//! That pairing is why this lives in one place. The reboot used to be raised
//! by each path that observed a terminal outcome for a *running* service: the
//! main job ending, a health check failing or timing out, the watchdog
//! expiring. The suppression, meanwhile, keyed on the cause and `ErrorControl`
//! alone and never checked that a reboot had been scheduled. So the two
//! disagreed for every path nobody had enumerated — a budget exhausted by
//! repeated `ReadinessTimeout`, `PreHookFailure`, `ParentSetupFailure` — and a
//! Critical service that could never get as far as running settled quietly in
//! Failed with neither escalation. The case that most needs one got neither.
//!
//! So: one predicate, read from the service table rather than from whichever
//! dispatch happened to notice, and a reconciliation pass that raises the
//! reboot however the service got there. Enumerating the producers is what
//! failed the first time, and a new failure path would fail it again.

use crate::boundary::ShutdownFinalizer;
use crate::service::ErrorControl;
use crate::service::ServiceTable;
use crate::service::runtime::{ServiceState, TransitionCause};

use super::dispatch::SupervisorShutdownFinalizationDispatch;
use super::state::{Supervisor, SupervisorError};

/// The service is Critical, has exhausted its restart budget, and is owed an
/// immediate reboot.
///
/// Read from the table rather than from a dispatch's transitions so that every
/// caller answers the same question. The transition has already been applied
/// by the time any dispatch carrying it exists, so the two are equivalent
/// where both are available — and the table is available where a dispatch is
/// not.
pub(super) fn critical_budget_reboot_owed(services: &ServiceTable, service: &str) -> bool {
    let Some(definition) = services.definition(service) else {
        return false;
    };
    if definition.error_control != ErrorControl::Critical {
        return false;
    }
    services.runtime(service).is_some_and(|runtime| {
        runtime.state == ServiceState::Failed
            && runtime.cause == Some(TransitionCause::RestartBudgetExhausted)
    })
}

/// What the reconciliation pass did, so the console can say why the machine is
/// going down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorCriticalBudgetRebootDispatch {
    pub service: String,
    /// What spent the last of the budget, where the path that observed it
    /// left the reboot to this pass (see [`CriticalRebootTrigger`]).
    pub trigger: CriticalRebootTrigger,
    /// When that was observed, if the path recorded a time.
    pub observed_at_ns: Option<u64>,
    pub finalization: SupervisorShutdownFinalizationDispatch,
}

/// What exhausted a Critical service's restart budget.
///
/// The paths that watch a running service — its main job ending, a health
/// check failing or timing out, the watchdog expiring — used to finalise the
/// reboot inline, and named themselves in the console line and the
/// `critical.failure` audit event. The runtime now leaves every reboot to the
/// end of its turn, so that the turn's console output is written before the
/// action that does not return (PEI-827); the trigger travels with it so the
/// operator and the audit log still learn what happened, not just that the
/// budget ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CriticalRebootTrigger {
    ServiceMainTerminal,
    HealthCheckFailure,
    WatchdogTimeout,
    /// Exhausted by a failure before the service ran — a readiness, hook or
    /// check timeout — where only the budget itself is the news.
    RestartBudgetExhausted,
}

impl CriticalRebootTrigger {
    /// The `trigger` field of the `critical.failure` audit event.
    pub fn kmes_id(self) -> &'static str {
        match self {
            Self::ServiceMainTerminal => "service_main_terminal",
            Self::HealthCheckFailure => "health_check_failure",
            Self::WatchdogTimeout => "watchdog_timeout",
            Self::RestartBudgetExhausted => "restart_budget_exhausted",
        }
    }

    /// The reason in the "critical service X failed" console line, or `None`
    /// where the budget line says all there is.
    pub fn console_reason(self) -> Option<&'static str> {
        match self {
            Self::ServiceMainTerminal => Some("service main exited"),
            Self::HealthCheckFailure => Some("health check failed"),
            Self::WatchdogTimeout => Some("watchdog timeout"),
            Self::RestartBudgetExhausted => None,
        }
    }
}

/// A Critical reboot a path observed but left to the reconciliation pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeferredCriticalReboot {
    pub service: String,
    pub trigger: CriticalRebootTrigger,
    pub observed_at_ns: Option<u64>,
}

/// A Critical reboot the reconciliation pass would raise now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriticalRebootOwed {
    pub service: String,
    pub trigger: CriticalRebootTrigger,
}

impl Supervisor {
    /// Remember a Critical reboot a path observed and was asked not to
    /// finalise, so the reconciliation pass can name what caused it.
    pub(super) fn note_deferred_critical_reboot(
        &mut self,
        service: &str,
        trigger: CriticalRebootTrigger,
        observed_at_ns: Option<u64>,
    ) {
        self.deferred_critical_reboot = Some(DeferredCriticalReboot {
            service: service.to_string(),
            trigger,
            observed_at_ns,
        });
    }

    /// The Critical reboot [`Self::process_due_critical_budget_reboot`] would
    /// raise if called now, without raising it.
    ///
    /// For the runtime to announce the reboot on the console before it
    /// happens: the action does not return, so anything said afterwards is
    /// never heard.
    pub fn critical_budget_reboot_owed(&self) -> Option<CriticalRebootOwed> {
        if self.shutdown().is_some() {
            return None;
        }
        let service = self
            .services
            .service_names()
            .into_iter()
            .find(|service| critical_budget_reboot_owed(&self.services, service))?
            .to_string();
        let trigger = self
            .deferred_critical_reboot
            .as_ref()
            .filter(|deferred| deferred.service == service)
            .map_or(CriticalRebootTrigger::RestartBudgetExhausted, |deferred| {
                deferred.trigger
            });
        Some(CriticalRebootOwed { service, trigger })
    }

    /// Reboot if a Critical service has exhausted its restart budget.
    ///
    /// Runs once per runtime turn, after the turn's events have been applied.
    /// The paths that raise the reboot inline have already set the shutdown
    /// state by then, so this finds nothing and does nothing; it exists for
    /// the paths that do not, which is all of the ones that fail a service
    /// before it ever runs.
    ///
    /// A failure during a shutdown does not reboot (§12.2 step 2): the system
    /// is already going down and rebooting from here would loop.
    pub fn process_due_critical_budget_reboot<F>(
        &mut self,
        finalizer: &mut F,
        now_ns: u64,
    ) -> Result<Option<SupervisorCriticalBudgetRebootDispatch>, SupervisorError>
    where
        F: ShutdownFinalizer + ?Sized,
    {
        let Some(owed) = self.critical_budget_reboot_owed() else {
            // Whatever was deferred is settled: either the shutdown that was
            // installed for it is under way, or the service has moved on.
            self.deferred_critical_reboot = None;
            return Ok(None);
        };
        let observed_at_ns = self
            .deferred_critical_reboot
            .take()
            .filter(|deferred| deferred.service == owed.service)
            .and_then(|deferred| deferred.observed_at_ns);
        let finalization = self.critical_reboot(finalizer, now_ns)?;
        Ok(Some(SupervisorCriticalBudgetRebootDispatch {
            service: owed.service,
            trigger: owed.trigger,
            observed_at_ns,
            finalization,
        }))
    }
}
