//! `boot:settled` — start a service once the boot set has stopped moving.
//!
//! The motivating case is a console login prompt. peinit writes progress to
//! `/dev/console` and a terminal-attached service writes its prompt to the same
//! device, so a prompt started mid-boot is written over: `login` emits
//! `Username: ` with no trailing newline and blocks, peinit appends
//! `peinit: service X started`, and the line reads as garbage until the user
//! presses Enter and login redraws.
//!
//! Waiting is a *scheduling* preference, not a dependency. login does not need
//! a quiet boot to function — it needs lpsd, which is what its `Requires` says.
//! Expressing "wait for quiet" as a dependency would overload that word and,
//! worse, would need synthesised edges from every boot service plus exclusion
//! rules to avoid a cycle. A trigger sits outside the dependency graph
//! entirely, so none of that arises.
//!
//! Settled means every service in the Phase 2 boot plan has reached a state it
//! will not leave on its own. **Or the deadline expires** — which is the half
//! that matters most in practice: a service that hangs in `Starting` forever
//! must not cost the operator their only way in, and a console prompt is needed
//! most precisely when something is broken. systemd caps its equivalent
//! (`Type=idle`) at 5s for the same reason.

use crate::service::ServiceTable;
use crate::service::runtime::ServiceState;

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// Seconds to wait for the boot set to settle before starting anyway.
///
/// Matches systemd's `Type=idle` cap. Long enough for an ordinary boot to
/// finish, short enough that a hung service delays the prompt by an annoyance
/// rather than denying it.
pub const DEFAULT_SETTLE_TIMEOUT_SECS: u32 = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BootSettleTracker {
    /// Boot-plan services whose movement we are waiting to stop.
    awaiting: Vec<String>,
    /// Services to start once it has, in definition order.
    pending_starts: Vec<String>,
    deadline_ns: u64,
    /// Due time used once settled: a real timestamp in the past. See
    /// [`next_deadline`](Self::next_deadline) for why this may never be zero.
    settled_due_at_ns: u64,
    fired: bool,
}

impl BootSettleTracker {
    pub fn configure_phase2(
        &mut self,
        services: &ServiceTable,
        awaiting: Vec<String>,
        observed_at_ns: u64,
        settle_timeout_secs: u32,
    ) {
        let mut pending_starts = services
            .service_names()
            .into_iter()
            .filter(|service| {
                services.definition(service).is_some_and(|definition| {
                    definition.has_boot_settled_trigger() && !definition.disabled
                })
            })
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        pending_starts.sort();

        self.awaiting = awaiting;
        self.pending_starts = pending_starts;
        self.deadline_ns = observed_at_ns
            .saturating_add(u64::from(settle_timeout_secs).saturating_mul(NANOS_PER_SEC));
        // `max(1)` guards the one case where the plan's own timestamp would be
        // an unusable due time: a monotonic clock reading exactly zero. Any
        // non-zero value in the past behaves identically, and zero does not.
        self.settled_due_at_ns = observed_at_ns.max(1);
        self.fired = false;
    }

    /// When to look again.
    ///
    /// Once the boot set has settled this returns a due time already in the
    /// past, so the next timer sync fires straight away rather than waiting out
    /// the deadline. The timer is re-armed after every turn, so a transition
    /// that settles the last service brings this forward without needing an
    /// event of its own.
    ///
    /// That past time is `settled_due_at_ns` — the moment the boot plan was
    /// made — and it is emphatically NOT zero. `timerfd_settime` reads an
    /// `it_value` of zero as *disarm*, so returning 0 for "already due" turns
    /// the timer off instead of firing it, and does so silently, because
    /// disarming succeeds. A deferred service then simply never starts.
    pub fn next_deadline(&self, services: &ServiceTable) -> Option<BootSettleDeadline> {
        if self.fired || self.pending_starts.is_empty() {
            return None;
        }
        let due_at_ns = if self.is_settled(services) {
            self.settled_due_at_ns
        } else {
            self.deadline_ns
        };
        Some(BootSettleDeadline {
            due_at_ns,
            services: self.pending_starts.clone(),
        })
    }

    /// The services to start, if the wait is over. Fires at most once.
    pub fn take_due(&mut self, services: &ServiceTable, now_ns: u64) -> Option<BootSettleDue> {
        let deadline = self.next_deadline(services)?;
        if deadline.due_at_ns > now_ns {
            return None;
        }
        self.fired = true;
        Some(BootSettleDue {
            services: deadline.services,
            timed_out: !self.is_settled(services),
        })
    }

    /// Every awaited service has reached a state it will not leave by itself.
    ///
    /// `Backoff` counts as unsettled — a service between restart attempts is
    /// still moving, and its next attempt will write to the console. A service
    /// missing from the table counts as settled: it cannot generate any more
    /// output, and waiting on something that is not there would hang until the
    /// deadline for no benefit.
    fn is_settled(&self, services: &ServiceTable) -> bool {
        self.awaiting.iter().all(|service| {
            services
                .runtime(service)
                .is_none_or(|runtime| is_terminal(runtime.state))
        })
    }
}

fn is_terminal(state: ServiceState) -> bool {
    match state {
        ServiceState::Active
        | ServiceState::Completed
        | ServiceState::Failed
        | ServiceState::Skipped
        | ServiceState::Abandoned
        | ServiceState::Inactive => true,
        ServiceState::Starting
        | ServiceState::Reloading
        | ServiceState::Stopping
        | ServiceState::Backoff => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootSettleDeadline {
    pub due_at_ns: u64,
    pub services: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootSettleDue {
    pub services: Vec<String>,
    /// The deadline expired rather than the boot set settling. Worth reporting:
    /// it means something is still moving, and whatever starts now may still
    /// have its output written over.
    pub timed_out: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::runtime::{ServiceTransition, TransitionCause};
    use crate::service::{ServiceDefinition, ServiceTrigger};

    const OBSERVED_AT_NS: u64 = 1_000;
    const TIMEOUT_SECS: u32 = 5;
    const DEADLINE_NS: u64 = OBSERVED_AT_NS + (TIMEOUT_SECS as u64) * NANOS_PER_SEC;

    fn table(definitions: Vec<ServiceDefinition>) -> ServiceTable {
        ServiceTable::from_boot_snapshot(definitions).expect("service table")
    }

    fn boot_service(name: &str) -> ServiceDefinition {
        ServiceDefinition::simple_system_boot(name, "/sbin/app")
    }

    fn settled_service(name: &str) -> ServiceDefinition {
        let mut definition = ServiceDefinition::simple_system_boot(name, "/bin/login");
        definition.triggers = vec![ServiceTrigger::BootSettled];
        definition
    }

    fn configured(services: &ServiceTable, awaiting: &[&str]) -> BootSettleTracker {
        let mut tracker = BootSettleTracker::default();
        tracker.configure_phase2(
            services,
            awaiting.iter().map(ToString::to_string).collect(),
            OBSERVED_AT_NS,
            TIMEOUT_SECS,
        );
        tracker
    }

    /// Walk a service to `state` through legal transitions — the state machine
    /// rejects shortcuts, and a test that fabricated states could assert
    /// against combinations peinit can never produce.
    fn set_state(services: &mut ServiceTable, service: &str, state: ServiceState) {
        let step = |services: &mut ServiceTable, to, cause| {
            services
                .transition_service(service, ServiceTransition { to, cause })
                .expect("legal transition");
        };
        if state == ServiceState::Inactive {
            return;
        }
        step(
            services,
            ServiceState::Starting,
            TransitionCause::ExplicitStart,
        );
        match state {
            ServiceState::Starting => {}
            ServiceState::Active | ServiceState::Completed => {
                step(services, state, TransitionCause::ExplicitStart)
            }
            ServiceState::Failed | ServiceState::Backoff => {
                step(services, state, TransitionCause::ParentSetupFailure)
            }
            ServiceState::Skipped => step(services, state, TransitionCause::ConditionSkipped),
            other => panic!("test helper does not walk to {other:?}"),
        }
    }

    /// With something still moving, the only thing that can end the wait is the
    /// deadline — so that is what is scheduled.
    #[test]
    fn an_unsettled_boot_waits_until_the_deadline() {
        let mut services = table(vec![boot_service("app"), settled_service("login-console")]);
        set_state(&mut services, "app", ServiceState::Starting);
        let tracker = configured(&services, &["app"]);

        let deadline = tracker.next_deadline(&services).expect("deadline");

        assert_eq!(deadline.due_at_ns, DEADLINE_NS);
        assert_eq!(deadline.services, vec!["login-console".to_string()]);
    }

    /// Once nothing is moving the wait is over immediately, rather than running
    /// out the clock. The timer is re-armed every turn, so a due time already
    /// in the past is how "now" is expressed.
    #[test]
    fn a_settled_boot_is_due_immediately() {
        let mut services = table(vec![boot_service("app"), settled_service("login-console")]);
        set_state(&mut services, "app", ServiceState::Active);
        let tracker = configured(&services, &["app"]);

        let deadline = tracker.next_deadline(&services).expect("deadline");
        assert!(deadline.due_at_ns <= OBSERVED_AT_NS, "must already be due");
        assert!(
            deadline.due_at_ns < DEADLINE_NS,
            "must not wait out the timeout"
        );
        // The bug this pins: `timerfd_settime` reads it_value == 0 as DISARM,
        // so a zero due time switches the timer off rather than firing it — and
        // silently, because disarming succeeds. The deferred service then never
        // starts at all.
        assert_ne!(
            deadline.due_at_ns, 0,
            "zero disarms the timer, it does not fire it"
        );
    }

    /// The same invariant, independent of settling: nothing this returns may
    /// ever be a due time that disarms the timer.
    #[test]
    fn a_due_time_is_never_zero() {
        for state in [
            ServiceState::Active,
            ServiceState::Starting,
            ServiceState::Failed,
            ServiceState::Backoff,
        ] {
            let mut services = table(vec![boot_service("app"), settled_service("login-console")]);
            set_state(&mut services, "app", state);
            let tracker = configured(&services, &["app"]);

            assert_ne!(
                tracker
                    .next_deadline(&services)
                    .expect("deadline")
                    .due_at_ns,
                0,
                "{state:?} produced a due time that would disarm the timer",
            );
        }
    }

    /// A clock reading zero must not produce a disarming due time either.
    #[test]
    fn a_zero_observation_time_still_yields_a_usable_due_time() {
        let mut services = table(vec![boot_service("app"), settled_service("login-console")]);
        set_state(&mut services, "app", ServiceState::Active);
        let mut tracker = BootSettleTracker::default();
        tracker.configure_phase2(&services, vec!["app".to_string()], 0, TIMEOUT_SECS);

        assert_ne!(
            tracker
                .next_deadline(&services)
                .expect("deadline")
                .due_at_ns,
            0
        );
    }

    /// The case the deadline exists for: a service that never leaves Starting
    /// must cost a delay, not the prompt.
    #[test]
    fn a_hung_service_yields_to_the_deadline() {
        let mut services = table(vec![boot_service("app"), settled_service("login-console")]);
        set_state(&mut services, "app", ServiceState::Starting);
        let mut tracker = configured(&services, &["app"]);

        assert!(tracker.take_due(&services, DEADLINE_NS - 1).is_none());

        let due = tracker.take_due(&services, DEADLINE_NS).expect("due");
        assert_eq!(due.services, vec!["login-console".to_string()]);
        assert!(
            due.timed_out,
            "the wait ended on the clock, not on settling"
        );
    }

    /// A failed boot service is as settled as a successful one: it has stopped
    /// moving, which is the only property this cares about.
    #[test]
    fn a_failed_service_counts_as_settled() {
        let mut services = table(vec![boot_service("app"), settled_service("login-console")]);
        set_state(&mut services, "app", ServiceState::Failed);
        let mut tracker = configured(&services, &["app"]);

        let due = tracker.take_due(&services, OBSERVED_AT_NS).expect("due");
        assert!(!due.timed_out);
    }

    /// A service between restart attempts is still moving — its next attempt
    /// will write to the console — so it must not count as settled.
    #[test]
    fn a_service_in_backoff_is_not_settled() {
        let mut services = table(vec![boot_service("app"), settled_service("login-console")]);
        set_state(&mut services, "app", ServiceState::Backoff);
        let tracker = configured(&services, &["app"]);

        assert_eq!(
            tracker
                .next_deadline(&services)
                .expect("deadline")
                .due_at_ns,
            DEADLINE_NS,
        );
    }

    #[test]
    fn firing_happens_once() {
        let mut services = table(vec![boot_service("app"), settled_service("login-console")]);
        set_state(&mut services, "app", ServiceState::Active);
        let mut tracker = configured(&services, &["app"]);

        assert!(tracker.take_due(&services, OBSERVED_AT_NS).is_some());
        assert!(tracker.take_due(&services, OBSERVED_AT_NS).is_none());
        assert!(tracker.next_deadline(&services).is_none());
    }

    /// Nothing waiting means no deadline at all — peinit must not hold a timer
    /// open, or wake up, for a boot with no deferred services.
    #[test]
    fn no_settled_services_schedules_nothing() {
        let services = table(vec![boot_service("app")]);
        let tracker = configured(&services, &["app"]);

        assert!(tracker.next_deadline(&services).is_none());
    }

    /// Disabled suppresses automatic activation by *any* trigger, and
    /// `boot:settled` is a trigger.
    #[test]
    fn a_disabled_service_is_not_started() {
        let mut login = settled_service("login-console");
        login.disabled = true;
        let services = table(vec![boot_service("app"), login]);
        let tracker = configured(&services, &["app"]);

        assert!(tracker.next_deadline(&services).is_none());
    }
}
