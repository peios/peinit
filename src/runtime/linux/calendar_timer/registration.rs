use crate::boundary::LinuxTimerFd;
use crate::runtime::{RuntimeEventRegistrar, RuntimeEventSource};
use crate::service::ServiceDefinition;
use crate::supervisor::Supervisor;
use crate::timer::boot::TimerBootRegistration;
use crate::timer::calendar::CalendarSchedule;

use super::entry::LinuxCalendarTimerEntry;
use super::error::LinuxCalendarTimerError;
use super::random::jittered_deadline_ns;

pub(super) fn definitions_from_supervisor(supervisor: &Supervisor) -> Vec<ServiceDefinition> {
    supervisor
        .services()
        .service_names()
        .into_iter()
        .filter_map(|service| supervisor.services().definition(service).cloned())
        .collect()
}

pub(super) fn register_calendar_timer<E>(
    registration: TimerBootRegistration,
    registrar: &mut E,
) -> Result<(RuntimeEventSource, LinuxCalendarTimerEntry), LinuxCalendarTimerError>
where
    E: RuntimeEventRegistrar + ?Sized,
{
    let calendar = CalendarSchedule::parse(&registration.schedule).map_err(|source| {
        LinuxCalendarTimerError::Parse {
            service: registration.service.clone(),
            schedule: registration.schedule.clone(),
            source,
        }
    })?;
    let timer = LinuxTimerFd::create_realtime().map_err(LinuxCalendarTimerError::Create)?;
    let armed_deadline_ns =
        jittered_deadline_ns(registration.next_scheduled_ns, registration.jitter_secs)?;
    timer
        .arm_realtime_absolute_ns(armed_deadline_ns)
        .map_err(LinuxCalendarTimerError::Arm)?;
    let fd = timer.as_raw_fd();
    let source = RuntimeEventSource::calendar_timer(fd).map_err(LinuxCalendarTimerError::Source)?;
    registrar
        .register_source(fd, source)
        .map_err(LinuxCalendarTimerError::Register)?;
    Ok((
        source,
        LinuxCalendarTimerEntry {
            timer,
            service: registration.service,
            schedule: registration.schedule,
            storage: registration.storage,
            persistent: registration.persistent,
            calendar,
            next_scheduled_ns: registration.next_scheduled_ns,
            armed_deadline_ns,
            jitter_secs: registration.jitter_secs,
        },
    ))
}
