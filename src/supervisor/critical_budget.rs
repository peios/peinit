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
    pub finalization: SupervisorShutdownFinalizationDispatch,
}

impl Supervisor {
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
        if self.shutdown().is_some() {
            return Ok(None);
        }
        let Some(service) = self
            .services
            .service_names()
            .into_iter()
            .find(|service| critical_budget_reboot_owed(&self.services, service))
            .map(ToString::to_string)
        else {
            return Ok(None);
        };
        let finalization = self.critical_reboot(finalizer, now_ns)?;
        Ok(Some(SupervisorCriticalBudgetRebootDispatch {
            service,
            finalization,
        }))
    }
}
