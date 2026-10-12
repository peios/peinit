use super::*;
use crate::boot::phase2::Phase2BootSettings;
use crate::runtime::linux::calendar_timer::test_support::{
    FixedClock, RecordingRegistrar, TimerHistory, ns_utc,
};
use crate::service::{ServiceDefinition, ServiceTrigger};
use crate::supervisor::SupervisorSettings;

fn service(schedules: &[&str]) -> ServiceDefinition {
    let mut service = ServiceDefinition::simple_system_boot("app", "/bin/true");
    service.triggers = schedules
        .iter()
        .map(|schedule| ServiceTrigger::Timer {
            schedule: (*schedule).into(),
        })
        .collect();
    service.timer_persistent = false;
    service.timer_jitter_secs = 10;
    service
}

fn supervisor(services: Vec<ServiceDefinition>, now: u64) -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(Phase2BootSettings::default()));
    supervisor
        .run_phase2_boot(&mut TimerHistory::new(services, 0), &mut clock(now))
        .unwrap();
    supervisor
}

fn clock(now: u64) -> FixedClock {
    FixedClock {
        monotonic_ns: 1,
        realtime_ns: now,
    }
}

#[test]
fn repeated_reload_preserves_fd_deadline_jitter_and_last_firing() {
    let now = ns_utc(2024, 5, 3, 12, 0, 0);
    let mut definition = service(&["daily UTC"]);
    let mut supervisor = supervisor(vec![definition.clone()], now);
    let mut table = LinuxCalendarTimerTable::new();
    let mut registrar = RecordingRegistrar::default();
    let sources = table
        .reconfigure_timers(&supervisor, &mut clock(now), &mut registrar)
        .unwrap();
    let (&fd, entry) = table.entries.iter_mut().next().unwrap();
    entry.last_fired_ns = Some(now - 1);
    let scheduled = entry.next_scheduled_ns;
    let deadline = entry.armed_deadline_ns;
    // An unrelated service edit also retains the armed timer.
    definition.image_path = "/bin/false".into();
    supervisor
        .reload_config_from_registry(&mut TimerHistory::new(vec![definition], 0))
        .unwrap();
    for time in [now + 1, scheduled + 1, deadline + 1] {
        assert_eq!(
            table
                .reconfigure_timers(&supervisor, &mut clock(time), &mut registrar)
                .unwrap(),
            sources
        );
        let entry = &table.entries[&fd];
        assert_eq!(entry.next_scheduled_ns, scheduled);
        assert_eq!(entry.armed_deadline_ns, deadline);
        assert_eq!(entry.last_fired_ns, Some(now - 1));
        assert_eq!(registrar.registrations.len(), 1);
    }
}

#[test]
fn a_pending_final_occurrence_survives_reload_after_its_scheduled_time() {
    let now = ns_utc(2024, 5, 3, 12, 0, 0);
    let supervisor = supervisor(vec![service(&["2024-05-03 12:00:01 UTC"])], now);
    let mut table = LinuxCalendarTimerTable::new();
    let mut registrar = RecordingRegistrar::default();
    let sources = table
        .reconfigure_timers(&supervisor, &mut clock(now), &mut registrar)
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(
        table
            .reconfigure_timers(&supervisor, &mut clock(now + 2_000_000_000), &mut registrar)
            .unwrap(),
        sources
    );
    assert!(table.rejected.is_empty());
}

#[test]
fn changed_timer_settings_replace_the_fd_and_removed_or_disabled_timers_unregister() {
    let now = ns_utc(2024, 5, 3, 12, 0, 0);
    let mut definition = service(&["daily UTC"]);
    let mut table = LinuxCalendarTimerTable::new();
    let mut registrar = RecordingRegistrar::default();
    table
        .reconfigure_timers(
            &supervisor(vec![definition.clone()], now),
            &mut clock(now),
            &mut registrar,
        )
        .unwrap();
    for change in 0..4 {
        let old_fd = *table.entries.keys().next().unwrap();
        match change {
            0 => definition.timer_jitter_secs += 1,
            1 => definition.timer_persistent = true,
            2 => definition.triggers.push(ServiceTrigger::Timer {
                schedule: "weekly UTC".into(),
            }),
            _ => {
                definition.triggers = vec![ServiceTrigger::Timer {
                    schedule: "hourly UTC".into(),
                }]
            }
        }
        table
            .reconfigure_timers(
                &supervisor(vec![definition.clone()], now),
                &mut clock(now + 1),
                &mut registrar,
            )
            .unwrap();
        assert!(
            !table.entries.contains_key(&old_fd),
            "changed setting {change}"
        );
        assert!(!registrar.registrations.contains_key(&old_fd));
        assert_eq!(table.entries.len(), registrar.registrations.len());
    }
    definition.disabled = true;
    table
        .reconfigure_timers(
            &supervisor(vec![definition], now),
            &mut clock(now),
            &mut registrar,
        )
        .unwrap();
    assert!(table.entries.is_empty());
    assert!(registrar.registrations.is_empty());

    table
        .reconfigure_timers(
            &supervisor(vec![service(&["daily UTC"])], now),
            &mut clock(now),
            &mut registrar,
        )
        .unwrap();
    table
        .reconfigure_timers(&supervisor(vec![], now), &mut clock(now), &mut registrar)
        .unwrap();
    assert!(table.entries.is_empty());
    assert!(registrar.registrations.is_empty());
}

#[test]
fn duplicate_schedules_retain_distinct_sources() {
    let now = ns_utc(2024, 5, 3, 12, 0, 0);
    let supervisor = supervisor(vec![service(&["daily UTC", "daily UTC"])], now);
    let mut table = LinuxCalendarTimerTable::new();
    let mut registrar = RecordingRegistrar::default();
    let sources = table
        .reconfigure_timers(&supervisor, &mut clock(now), &mut registrar)
        .unwrap();
    assert_eq!(sources.len(), 2);
    let reloaded = table
        .reconfigure_timers(&supervisor, &mut clock(now + 1), &mut registrar)
        .unwrap();
    assert_eq!(reloaded.len(), sources.len());
    assert!(sources.iter().all(|source| reloaded.contains(source)));
    assert_eq!(registrar.registrations.len(), 2);
}

#[test]
fn failed_new_registration_keeps_old_timers_and_removes_partial_replacements() {
    use crate::runtime::RuntimeEventRegistrationError;
    struct FailSecondNew {
        inner: RecordingRegistrar,
        calls: usize,
    }
    impl RuntimeEventRegistrar for FailSecondNew {
        fn register_source(
            &mut self,
            fd: i32,
            source: RuntimeEventSource,
        ) -> Result<(), RuntimeEventRegistrationError> {
            self.calls += 1;
            if self.calls == 2 {
                return Err(RuntimeEventRegistrationError::Register {
                    fd,
                    source,
                    message: "injected".into(),
                });
            }
            self.inner.register_source(fd, source)
        }
        fn unregister_source(&mut self, fd: i32) -> Result<(), RuntimeEventRegistrationError> {
            self.inner.unregister_source(fd)
        }
    }
    let now = ns_utc(2024, 5, 3, 12, 0, 0);
    let definition = service(&["daily UTC"]);
    let mut table = LinuxCalendarTimerTable::new();
    let mut registrar = RecordingRegistrar::default();
    let sources = table
        .reconfigure_timers(
            &supervisor(vec![definition.clone()], now),
            &mut clock(now),
            &mut registrar,
        )
        .unwrap();
    let before = table.views();
    let mut added = service(&["daily UTC", "weekly UTC"]);
    added.name = "new".into();
    let mut registrar = FailSecondNew {
        inner: registrar,
        calls: 0,
    };
    assert!(
        table
            .reconfigure_timers(
                &supervisor(vec![definition, added], now),
                &mut clock(now),
                &mut registrar
            )
            .is_err()
    );
    assert_eq!(table.views(), before);
    assert_eq!(
        registrar
            .inner
            .registrations
            .values()
            .copied()
            .collect::<Vec<_>>(),
        sources
    );
}
