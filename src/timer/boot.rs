use crate::boundary::{BoundaryError, RegistryClient};
use crate::service::{ServiceDefinition, ServiceTrigger};

use super::calendar::{CalendarNextError, CalendarParseError, CalendarSchedule};
use super::state::TimerLastRunStorage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerBootPlan {
    pub registrations: Vec<TimerBootRegistration>,
    /// Triggers whose schedule will not parse or has no future occurrence.
    ///
    /// Per-trigger, not fatal. The planner used to propagate the first of
    /// these with `?`, which at boot aborted registration of *every* timer and
    /// dropped the machine into the recovery console, and on reload came out
    /// of the run loop and ended PID 1's event loop — reachable without anyone
    /// running `reload-config`, since any drained registry watch event
    /// triggers a reload.
    ///
    /// The code already had the right policy elsewhere: graph validation
    /// treats an invalid schedule as a per-service `ValidationError`, blocks
    /// that service and continues. This matches it, so which policy applies no
    /// longer depends on whether the schedule fails at validation or at
    /// timerfd registration.
    pub rejected: Vec<TimerBootPlanError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerBootRegistration {
    pub service: String,
    pub schedule: String,
    pub storage: TimerLastRunStorage,
    pub persistent: bool,
    pub jitter_secs: u64,
    pub missed_firing: bool,
    pub next_scheduled_ns: u64,
    pub last_run_ns: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerBootPlanError {
    Registry {
        service: String,
        schedule: String,
        source: BoundaryError,
    },
    ParseSchedule {
        service: String,
        schedule: String,
        source: CalendarParseError,
    },
    ComputeNext {
        service: String,
        schedule: String,
        source: CalendarNextError,
    },
}

pub fn plan_timer_boot<R>(
    services: &[ServiceDefinition],
    registry: &mut R,
    now_realtime_ns: u64,
) -> Result<TimerBootPlan, TimerBootPlanError>
where
    R: RegistryClient + ?Sized,
{
    let mut registrations = Vec::new();
    let mut rejected = Vec::new();
    for service in services.iter().filter(|service| !service.disabled) {
        let schedules = timer_schedules(service);
        let storage = if schedules.len() == 1 {
            TimerLastRunStorage::SingleTimer
        } else {
            TimerLastRunStorage::PerTrigger
        };

        for schedule in schedules {
            match plan_timer_trigger(service, schedule, storage, registry, now_realtime_ns) {
                Ok(registration) => registrations.push(registration),
                // A registry read failure is infrastructure, not a bad
                // definition: it means registryd is in trouble, which is not
                // this service's fault and will not be fixed by skipping it.
                Err(error @ TimerBootPlanError::Registry { .. }) => return Err(error),
                Err(error) => rejected.push(error),
            }
        }
    }

    Ok(TimerBootPlan {
        registrations,
        rejected,
    })
}

pub fn plan_timer_reload(
    services: &[ServiceDefinition],
    now_realtime_ns: u64,
) -> Result<TimerBootPlan, TimerBootPlanError> {
    let mut registrations = Vec::new();
    let mut rejected = Vec::new();
    for service in services.iter().filter(|service| !service.disabled) {
        let schedules = timer_schedules(service);
        let storage = if schedules.len() == 1 {
            TimerLastRunStorage::SingleTimer
        } else {
            TimerLastRunStorage::PerTrigger
        };

        for schedule in schedules {
            match plan_reload_trigger(service, schedule, storage, now_realtime_ns) {
                Ok(registration) => registrations.push(registration),
                Err(error) => rejected.push(error),
            }
        }
    }

    Ok(TimerBootPlan {
        registrations,
        rejected,
    })
}

fn plan_reload_trigger(
    service: &ServiceDefinition,
    schedule: &str,
    storage: TimerLastRunStorage,
    now_realtime_ns: u64,
) -> Result<TimerBootRegistration, TimerBootPlanError> {
    let calendar =
        CalendarSchedule::parse(schedule).map_err(|source| TimerBootPlanError::ParseSchedule {
            service: service.name.clone(),
            schedule: schedule.to_string(),
            source,
        })?;
    Ok(TimerBootRegistration {
        service: service.name.clone(),
        schedule: schedule.to_string(),
        storage,
        persistent: service.timer_persistent,
        jitter_secs: service.timer_jitter_secs,
        missed_firing: false,
        next_scheduled_ns: next_after(&service.name, schedule, &calendar, now_realtime_ns)?,
        last_run_ns: None,
    })
}

fn plan_timer_trigger<R>(
    service: &ServiceDefinition,
    schedule: &str,
    storage: TimerLastRunStorage,
    registry: &mut R,
    now_realtime_ns: u64,
) -> Result<TimerBootRegistration, TimerBootPlanError>
where
    R: RegistryClient + ?Sized,
{
    let calendar =
        CalendarSchedule::parse(schedule).map_err(|source| TimerBootPlanError::ParseSchedule {
            service: service.name.clone(),
            schedule: schedule.to_string(),
            source,
        })?;

    let (last_run_ns, missed_firing, next_scheduled_ns) = if service.timer_persistent {
        let last_run_ns = registry
            .read_timer_last_run(&service.name, schedule, storage)
            .map_err(|source| TimerBootPlanError::Registry {
                service: service.name.clone(),
                schedule: schedule.to_string(),
                source,
            })?;
        persistent_boot_decision(
            &service.name,
            schedule,
            &calendar,
            last_run_ns,
            now_realtime_ns,
        )?
    } else {
        (
            None,
            false,
            next_after(&service.name, schedule, &calendar, now_realtime_ns)?,
        )
    };

    Ok(TimerBootRegistration {
        service: service.name.clone(),
        schedule: schedule.to_string(),
        storage,
        persistent: service.timer_persistent,
        jitter_secs: service.timer_jitter_secs,
        missed_firing,
        next_scheduled_ns,
        last_run_ns,
    })
}

fn persistent_boot_decision(
    service: &str,
    schedule: &str,
    calendar: &CalendarSchedule,
    last_run_ns: Option<u64>,
    now_realtime_ns: u64,
) -> Result<(Option<u64>, bool, u64), TimerBootPlanError> {
    let Some(last_run_ns) = last_run_ns else {
        return Ok((
            None,
            true,
            next_after(service, schedule, calendar, now_realtime_ns)?,
        ));
    };

    if last_run_ns > now_realtime_ns {
        return Ok((
            Some(last_run_ns),
            true,
            next_after(service, schedule, calendar, now_realtime_ns)?,
        ));
    }

    let next_after_last_run = next_after(service, schedule, calendar, last_run_ns)?;
    if next_after_last_run <= now_realtime_ns {
        Ok((
            Some(last_run_ns),
            true,
            next_after(service, schedule, calendar, now_realtime_ns)?,
        ))
    } else {
        Ok((Some(last_run_ns), false, next_after_last_run))
    }
}

fn next_after(
    service: &str,
    schedule: &str,
    calendar: &CalendarSchedule,
    after_realtime_ns: u64,
) -> Result<u64, TimerBootPlanError> {
    calendar
        .next_after_ns(after_realtime_ns)
        .map_err(|source| TimerBootPlanError::ComputeNext {
            service: service.to_string(),
            schedule: schedule.to_string(),
            source,
        })
}

fn timer_schedules(service: &ServiceDefinition) -> Vec<&str> {
    service
        .triggers
        .iter()
        .filter_map(|trigger| match trigger {
            ServiceTrigger::Timer { schedule } => Some(schedule.as_str()),
            ServiceTrigger::Boot | ServiceTrigger::BootSettled | ServiceTrigger::Other(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
