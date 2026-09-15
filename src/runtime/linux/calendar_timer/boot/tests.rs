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
