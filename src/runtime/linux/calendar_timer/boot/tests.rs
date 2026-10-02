use super::*;
use crate::boot::phase2::Phase2BootSettings;
use crate::boundary::TimerLastRunWriteOutcome;
use crate::operation::OperationSource;
use crate::runtime::linux::calendar_timer::test_support::{
    FixedClock, RecordingRegistrar, TimerHistory, TimerWriteRecorder, ns_utc,
};
use crate::service::runtime::ServiceState;
use crate::service::{ServiceDefinition, ServiceTrigger};
use crate::supervisor::{SupervisorSettings, SupervisorTimerAction};

#[test]
fn boot_persistent_catch_up_returns_timer_turn_and_keeps_last_run_write_best_effort() {
    let mut service = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    service.triggers = vec![ServiceTrigger::Timer {
        schedule: "daily UTC".to_string(),
    }];
    service.timer_persistent = true;

    let now_ns = ns_utc(2024, 5, 3, 12, 0, 0);
    let last_run_ns = ns_utc(2024, 5, 1, 0, 0, 0);
    let mut supervisor = Supervisor::new(SupervisorSettings::new(Phase2BootSettings {
        max_parallel_starts: 10,
        ..Phase2BootSettings::default()
    }));
    let mut registry = TimerHistory::new(vec![service], last_run_ns);
    let mut boot_clock = FixedClock {
        monotonic_ns: 1,
        realtime_ns: now_ns,
    };
    supervisor
        .run_phase2_boot(&mut registry, &mut boot_clock)
        .expect("boot supervisor");
    let mut table = LinuxCalendarTimerTable::new();
    let mut clock = FixedClock {
        monotonic_ns: 10_000,
        realtime_ns: now_ns,
    };
    let mut registrar = RecordingRegistrar::default();
    let mut writer = TimerWriteRecorder::default();

    let registration = table
        .register_boot_timers(
            &mut supervisor,
            &mut clock,
            &mut registry,
            &mut writer,
            &mut registrar,
        )
        .expect("register boot timers");

    assert_eq!(registration.sources.len(), 1);
    assert_eq!(registration.catch_up_turns.len(), 1);
    assert_eq!(writer.writes.len(), 1);
    assert_eq!(writer.writes[0].timestamp_realtime_ns, now_ns);
    assert_eq!(
        supervisor.services().runtime("app").expect("runtime").state,
        ServiceState::Starting,
    );
    let (_, turn) = &registration.catch_up_turns[0];
    let RuntimeCalendarTimerTurn::Read {
        supervisor: Some(dispatch),
        last_run_write: Some(Ok(TimerLastRunWriteOutcome::Queued { pid: 4242 })),
        next_scheduled_ns: Some(_),
        ..
    } = turn
    else {
        panic!("expected catch-up timer read turn");
    };
    let SupervisorTimerAction::Start { outcome, .. } = &dispatch.action else {
        panic!("expected timer start");
    };
    assert_eq!(
        outcome.plan.requested_operation_source,
        OperationSource::Timer
    );
}

/// A persistent daily timer booted at `now_ns` with `last_run_ns` recorded:
/// the supervisor, and the table registered for it.
fn booted_daily(last_run_ns: u64, now_ns: u64) -> (Supervisor, LinuxCalendarTimerTable) {
    let mut service = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    service.triggers = vec![ServiceTrigger::Timer {
        schedule: "daily UTC".to_string(),
    }];
    service.timer_persistent = true;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(Phase2BootSettings {
        max_parallel_starts: 10,
        ..Phase2BootSettings::default()
    }));
    let mut registry = TimerHistory::new(vec![service], last_run_ns);
    let mut clock = FixedClock {
        monotonic_ns: 1,
        realtime_ns: now_ns,
    };
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let mut table = LinuxCalendarTimerTable::new();
    table
        .register_boot_timers(
            &mut supervisor,
            &mut clock,
            &mut registry,
            &mut TimerWriteRecorder::default(),
            &mut RecordingRegistrar::default(),
        )
        .expect("register boot timers");
    (supervisor, table)
}

fn last_fired(table: &LinuxCalendarTimerTable) -> Option<u64> {
    match &table.views()["app"][0].arming {
        crate::control::query::ServiceTimerArming::Armed { last_fired_ns, .. } => *last_fired_ns,
        other => panic!("expected an armed timer, got {other:?}"),
    }
}

/// §9.2: a persistent timer's last firing is what was recorded before the
/// boot, until it fires again; one caught up at boot last fired then.
#[test]
fn a_persistent_timers_last_firing_is_seeded_at_boot_from_its_record() {
    let now_ns = ns_utc(2024, 5, 3, 12, 0, 0);
    // Ran at midnight today: nothing missed.
    let ran_ns = ns_utc(2024, 5, 3, 0, 0, 5);
    let (_, table) = booted_daily(ran_ns, now_ns);
    assert_eq!(last_fired(&table), Some(ran_ns));
    // Ran two days ago: caught up now, and that is its last firing.
    let (_, table) = booted_daily(ns_utc(2024, 5, 1, 0, 0, 5), now_ns);
    assert_eq!(last_fired(&table), Some(now_ns));
}

/// A reload re-arms from now and reads no history (§9.3), but a trigger it
/// keeps keeps when it last fired.
#[test]
fn a_reload_keeps_when_a_kept_trigger_last_fired() {
    let now_ns = ns_utc(2024, 5, 3, 12, 0, 0);
    let ran_ns = ns_utc(2024, 5, 3, 0, 0, 5);
    let (supervisor, mut table) = booted_daily(ran_ns, now_ns);
    let mut clock = FixedClock {
        monotonic_ns: 20_000,
        realtime_ns: now_ns + 60_000_000_000,
    };
    table
        .reconfigure_timers(&supervisor, &mut clock, &mut RecordingRegistrar::default())
        .expect("reconfigure");
    assert_eq!(last_fired(&table), Some(ran_ns));
}
