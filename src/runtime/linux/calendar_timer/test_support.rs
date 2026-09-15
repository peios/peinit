//! Fakes shared by the calendar timer tests: a clock that reads whatever it
//! was told, a registry with one last-run timestamp, a writer that records
//! the writes it was asked for, and a registrar that records sources.

use std::collections::BTreeMap;

use jiff::civil::DateTime;
use jiff::tz::TimeZone;

use crate::boundary::{
    BoundaryError, RegistryClient, TimerLastRunWriteOutcome, TimerLastRunWriteRequest,
    TimerLastRunWriter,
};
use crate::runtime::{RuntimeEventRegistrar, RuntimeEventRegistrationError, RuntimeEventSource};
use crate::service::ServiceDefinition;
use crate::timer::state::TimerLastRunStorage;

#[derive(Debug)]
pub(super) struct FixedClock {
    pub(super) monotonic_ns: u64,
    pub(super) realtime_ns: u64,
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
pub(super) struct TimerHistory {
    services: Vec<ServiceDefinition>,
    last_run_ns: u64,
}

impl TimerHistory {
    pub(super) fn new(services: Vec<ServiceDefinition>, last_run_ns: u64) -> Self {
        Self {
            services,
            last_run_ns,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimerWrite {
    pub(super) timestamp_realtime_ns: u64,
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
pub(super) struct TimerWriteRecorder {
    pub(super) writes: Vec<TimerWrite>,
}

impl TimerLastRunWriter for TimerWriteRecorder {
    fn queue_timer_last_run_write(
        &mut self,
        request: TimerLastRunWriteRequest,
    ) -> Result<TimerLastRunWriteOutcome, BoundaryError> {
        self.writes.push(TimerWrite {
            timestamp_realtime_ns: request.timestamp_realtime_ns,
        });
        Ok(TimerLastRunWriteOutcome::Queued { pid: 4242 })
    }
}

#[derive(Debug, Default)]
pub(super) struct RecordingRegistrar {
    pub(super) registrations: BTreeMap<i32, RuntimeEventSource>,
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

pub(super) fn ns_utc(year: i16, month: i8, day: i8, hour: i8, minute: i8, second: i8) -> u64 {
    let nanos = DateTime::new(year, month, day, hour, minute, second, 0)
        .expect("datetime")
        .to_zoned(TimeZone::UTC)
        .expect("utc")
        .timestamp()
        .as_nanosecond();
    u64::try_from(nanos).expect("positive timestamp")
}
