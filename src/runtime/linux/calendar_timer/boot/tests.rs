use std::collections::BTreeMap;

use jiff::civil::DateTime;
use jiff::tz::TimeZone;

use super::*;
use crate::boot::phase2::Phase2BootSettings;
use crate::boundary::{
    BoundaryError, RegistryClient, TimerLastRunWriteOutcome, TimerLastRunWriteRequest,
    TimerLastRunWriter,
};
use crate::operation::OperationSource;
use crate::runtime::RuntimeEventRegistrationError;
use crate::service::runtime::ServiceState;
use crate::service::{ServiceDefinition, ServiceTrigger};
use crate::supervisor::{SupervisorSettings, SupervisorTimerAction};
use crate::timer::state::TimerLastRunStorage;

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
        last_run_write: Some(Ok(TimerLastRunWriteOutcome::Queued)),
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

#[derive(Debug)]
struct FixedClock {
    monotonic_ns: u64,
    realtime_ns: u64,
}

impl crate::boundary::Clock for FixedClock {
    fn monotonic_ns(&mut self) -> Result<u64, BoundaryError> {
        Ok(self.monotonic_ns)
    }
}

impl crate::boundary::RealtimeClock for FixedClock {
    fn realtime_ns(&mut self) -> Result<u64, BoundaryError> {
        Ok(self.realtime_ns)
    }
}

#[derive(Debug)]
struct TimerHistory {
    services: Vec<ServiceDefinition>,
    last_run_ns: u64,
}

impl TimerHistory {
    fn new(services: Vec<ServiceDefinition>, last_run_ns: u64) -> Self {
        Self {
            services,
            last_run_ns,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimerWrite {
    timestamp_realtime_ns: u64,
}

impl RegistryClient for TimerHistory {
    fn read_service_definitions(&mut self) -> Result<Vec<ServiceDefinition>, BoundaryError> {
        Ok(self.services.clone())
    }

    fn read_timer_last_run(
        &mut self,
        _service: &str,
        _schedule: &str,
        _storage: TimerLastRunStorage,
    ) -> Result<Option<u64>, BoundaryError> {
        Ok(Some(self.last_run_ns))
    }
}

#[derive(Debug, Default)]
struct TimerWriteRecorder {
    writes: Vec<TimerWrite>,
}

impl TimerLastRunWriter for TimerWriteRecorder {
    fn queue_timer_last_run_write(
        &mut self,
        request: TimerLastRunWriteRequest,
    ) -> Result<TimerLastRunWriteOutcome, BoundaryError> {
        self.writes.push(TimerWrite {
            timestamp_realtime_ns: request.timestamp_realtime_ns,
        });
        Ok(TimerLastRunWriteOutcome::Queued)
    }
}

#[derive(Debug, Default)]
struct RecordingRegistrar {
    registrations: BTreeMap<i32, RuntimeEventSource>,
}

impl RuntimeEventRegistrar for RecordingRegistrar {
    fn register_source(
        &mut self,
        fd: i32,
        source: RuntimeEventSource,
    ) -> Result<(), RuntimeEventRegistrationError> {
        self.registrations.insert(fd, source);
        Ok(())
    }

    fn unregister_source(&mut self, fd: i32) -> Result<(), RuntimeEventRegistrationError> {
        self.registrations.remove(&fd);
        Ok(())
    }
}

fn ns_utc(year: i16, month: i8, day: i8, hour: i8, minute: i8, second: i8) -> u64 {
    let nanos = DateTime::new(year, month, day, hour, minute, second, 0)
        .expect("datetime")
        .to_zoned(TimeZone::UTC)
        .expect("utc")
        .timestamp()
        .as_nanosecond();
    u64::try_from(nanos).expect("positive timestamp")
}
