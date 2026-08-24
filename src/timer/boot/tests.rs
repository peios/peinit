use std::collections::BTreeMap;

use jiff::civil::DateTime;
use jiff::tz::TimeZone;

use crate::boundary::{BoundaryError, RegistryClient};
use crate::service::{ServiceDefinition, ServiceTrigger};
use crate::timer::state::TimerLastRunStorage;

use super::{CalendarNextError, TimerBootPlanError, plan_timer_boot, plan_timer_reload};

#[test]
fn non_persistent_timer_ignores_history_and_uses_next_future_occurrence() {
    let mut registry = TimerHistory::default();
    registry.insert(
        "backup",
        "*-*-* 02:00 UTC",
        TimerLastRunStorage::SingleTimer,
        1,
    );
    let mut service = timer_service("backup", ["*-*-* 02:00 UTC"]);
    service.timer_persistent = false;

    let plan =
        plan_timer_boot(&[service], &mut registry, ns_utc(2024, 5, 1, 12, 0, 0)).expect("plan");

    assert_eq!(registry.reads.len(), 0);
    assert_eq!(plan.registrations.len(), 1);
    assert!(!plan.registrations[0].missed_firing);
    assert_eq!(
        plan.registrations[0].next_scheduled_ns,
        ns_utc(2024, 5, 2, 2, 0, 0),
    );
}

#[test]
fn persistent_missing_history_fires_once_and_registers_next_future() {
    let mut registry = TimerHistory::default();
    let service = timer_service("backup", ["*-*-* 02:00 UTC"]);

    let plan =
        plan_timer_boot(&[service], &mut registry, ns_utc(2024, 5, 1, 12, 0, 0)).expect("plan");

    assert_eq!(
        registry.reads,
        vec![(
            "backup".to_string(),
            "*-*-* 02:00 UTC".to_string(),
            TimerLastRunStorage::SingleTimer,
        )],
    );
    assert!(plan.registrations[0].missed_firing);
    assert_eq!(
        plan.registrations[0].next_scheduled_ns,
        ns_utc(2024, 5, 2, 2, 0, 0),
    );
}

#[test]
fn persistent_history_without_missed_occurrence_registers_existing_next() {
    let mut registry = TimerHistory::default();
    registry.insert(
        "backup",
        "*-*-* 02:00 UTC",
        TimerLastRunStorage::SingleTimer,
        ns_utc(2024, 5, 1, 2, 0, 0),
    );
    let service = timer_service("backup", ["*-*-* 02:00 UTC"]);

    let plan =
        plan_timer_boot(&[service], &mut registry, ns_utc(2024, 5, 1, 12, 0, 0)).expect("plan");

    assert!(!plan.registrations[0].missed_firing);
    assert_eq!(
        plan.registrations[0].next_scheduled_ns,
        ns_utc(2024, 5, 2, 2, 0, 0),
    );
}

#[test]
fn persistent_missed_history_fires_once_not_once_per_occurrence() {
    let mut registry = TimerHistory::default();
    registry.insert(
        "backup",
        "*-*-* 02:00 UTC",
        TimerLastRunStorage::SingleTimer,
        ns_utc(2024, 4, 27, 2, 0, 0),
    );
    let service = timer_service("backup", ["*-*-* 02:00 UTC"]);

    let plan =
        plan_timer_boot(&[service], &mut registry, ns_utc(2024, 5, 1, 12, 0, 0)).expect("plan");

    assert!(plan.registrations[0].missed_firing);
    assert_eq!(
        plan.registrations[0].next_scheduled_ns,
        ns_utc(2024, 5, 2, 2, 0, 0),
    );
}

#[test]
fn future_last_run_is_unknown_history_and_fires_once() {
    let mut registry = TimerHistory::default();
    registry.insert(
        "backup",
        "*-*-* 02:00 UTC",
        TimerLastRunStorage::SingleTimer,
        ns_utc(2024, 5, 4, 2, 0, 0),
    );
    let service = timer_service("backup", ["*-*-* 02:00 UTC"]);

    let plan =
        plan_timer_boot(&[service], &mut registry, ns_utc(2024, 5, 1, 12, 0, 0)).expect("plan");

    assert!(plan.registrations[0].missed_firing);
    assert_eq!(
        plan.registrations[0].next_scheduled_ns,
        ns_utc(2024, 5, 2, 2, 0, 0),
    );
}

#[test]
fn multiple_timers_use_per_trigger_history() {
    let mut registry = TimerHistory::default();
    let service = timer_service("backup", ["*-*-* 02:00 UTC", "*-*-* 14:00 UTC"]);

    let plan =
        plan_timer_boot(&[service], &mut registry, ns_utc(2024, 5, 1, 12, 0, 0)).expect("plan");

    assert_eq!(plan.registrations.len(), 2);
    assert_eq!(
        plan.registrations[0].storage,
        TimerLastRunStorage::PerTrigger
    );
    assert_eq!(
        registry.reads,
        vec![
            (
                "backup".to_string(),
                "*-*-* 02:00 UTC".to_string(),
                TimerLastRunStorage::PerTrigger,
            ),
            (
                "backup".to_string(),
                "*-*-* 14:00 UTC".to_string(),
                TimerLastRunStorage::PerTrigger,
            ),
        ],
    );
}

#[test]
fn disabled_services_do_not_register_timers() {
    let mut registry = TimerHistory::default();
    let mut service = timer_service("backup", ["*-*-* 02:00 UTC"]);
    service.disabled = true;

    let plan =
        plan_timer_boot(&[service], &mut registry, ns_utc(2024, 5, 1, 12, 0, 0)).expect("plan");

    assert!(plan.registrations.is_empty());
    assert!(registry.reads.is_empty());
}

#[test]
fn reload_timer_plan_arms_future_deadline_without_history_catchup() {
    let service = timer_service("backup", ["*-*-* 02:00 UTC"]);

    let plan = plan_timer_reload(&[service], ns_utc(2024, 5, 1, 12, 0, 0)).expect("reload plan");

    assert_eq!(plan.registrations.len(), 1);
    assert!(!plan.registrations[0].missed_firing);
    assert_eq!(plan.registrations[0].last_run_ns, None);
    assert_eq!(
        plan.registrations[0].next_scheduled_ns,
        ns_utc(2024, 5, 2, 2, 0, 0),
    );
}

#[test]
fn invalid_schedule_reports_service_and_schedule() {
    let mut registry = TimerHistory::default();
    let service = timer_service("backup", ["*-*-* 02:00:00.5 UTC"]);

    let plan = plan_timer_boot(&[service], &mut registry, ns_utc(2024, 5, 1, 12, 0, 0))
        .expect("a bad schedule must not fail the whole plan");

    assert!(plan.registrations.is_empty());
    assert!(matches!(
        &plan.rejected[..],
        [TimerBootPlanError::ParseSchedule {
            service,
            schedule,
            ..
        }] if service == "backup" && schedule == "*-*-* 02:00:00.5 UTC"
    ));
}

#[test]
fn one_bad_schedule_does_not_stop_the_others_arming() {
    // The whole point: at boot this used to abort registration of every timer
    // and drop the machine into recovery; on reload it ended PID 1's loop.
    let mut registry = TimerHistory::default();
    let services = [
        timer_service("early", ["*-*-* 02:00 UTC"]),
        timer_service("broken", ["*-*-* 02:00:00.5 UTC"]),
        timer_service("late", ["*-*-* 03:00 UTC"]),
    ];
    let now = ns_utc(2024, 5, 1, 12, 0, 0);

    for (label, plan) in [
        (
            "boot",
            plan_timer_boot(&services, &mut registry, now).expect("boot plan"),
        ),
        (
            "reload",
            plan_timer_reload(&services, now).expect("reload plan"),
        ),
    ] {
        assert_eq!(
            plan.registrations
                .iter()
                .map(|registration| registration.service.as_str())
                .collect::<Vec<_>>(),
            vec!["early", "late"],
            "{label} dropped the healthy timers"
        );
        assert_eq!(plan.rejected.len(), 1, "{label} rejections");
    }
}

#[test]
fn a_schedule_that_can_never_match_is_reported_rather_than_walked() {
    // Parses fine, matches nothing: February has no 30th. This used to walk
    // roughly 2.9 million days before saying so.
    let mut registry = TimerHistory::default();
    let service = timer_service("impossible", ["*-02-30 02:00 UTC"]);

    let plan = plan_timer_boot(&[service], &mut registry, ns_utc(2024, 5, 1, 12, 0, 0))
        .expect("an unsatisfiable schedule must not fail the whole plan");

    assert!(plan.registrations.is_empty());
    assert!(matches!(
        &plan.rejected[..],
        [TimerBootPlanError::ComputeNext {
            service,
            source: CalendarNextError::NoFutureOccurrence,
            ..
        }] if service == "impossible"
    ));
}

#[test]
fn a_leap_day_schedule_still_arms_across_the_horizon() {
    // The sparsest legitimate schedule there is: 29 February. It must survive
    // the search bound, or the bound has broken real configurations.
    let mut registry = TimerHistory::default();
    let service = timer_service("leap", ["*-02-29 02:00 UTC"]);

    let plan = plan_timer_boot(&[service], &mut registry, ns_utc(2025, 3, 1, 12, 0, 0))
        .expect("leap plan");

    assert_eq!(plan.rejected.len(), 0);
    assert_eq!(
        plan.registrations[0].next_scheduled_ns,
        ns_utc(2028, 2, 29, 2, 0, 0),
    );
}

#[derive(Default)]
struct TimerHistory {
    values: BTreeMap<(String, String, TimerLastRunStorage), u64>,
    reads: Vec<(String, String, TimerLastRunStorage)>,
}

impl TimerHistory {
    fn insert(
        &mut self,
        service: &str,
        schedule: &str,
        storage: TimerLastRunStorage,
        timestamp_ns: u64,
    ) {
        self.values.insert(
            (service.to_string(), schedule.to_string(), storage),
            timestamp_ns,
        );
    }
}

impl RegistryClient for TimerHistory {
    fn read_service_definitions(&mut self) -> Result<Vec<ServiceDefinition>, BoundaryError> {
        Ok(Vec::new())
    }

    fn read_timer_last_run(
        &mut self,
        service: &str,
        schedule: &str,
        storage: TimerLastRunStorage,
    ) -> Result<Option<u64>, BoundaryError> {
        let key = (service.to_string(), schedule.to_string(), storage);
        self.reads.push(key.clone());
        Ok(self.values.get(&key).copied())
    }
}

fn timer_service<const N: usize>(name: &str, schedules: [&str; N]) -> ServiceDefinition {
    let mut service = ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"));
    service.triggers = schedules
        .into_iter()
        .map(|schedule| ServiceTrigger::Timer {
            schedule: schedule.to_string(),
        })
        .collect();
    service
}

fn ns_utc(year: i16, month: i8, day: i8, hour: i8, minute: i8, second: i8) -> u64 {
    let nanos = DateTime::new(year, month, day, hour, minute, second, 0)
        .expect("datetime")
        .to_zoned(TimeZone::UTC)
        .expect("zoned")
        .timestamp()
        .as_nanosecond();
    u64::try_from(nanos).expect("positive timestamp")
}
