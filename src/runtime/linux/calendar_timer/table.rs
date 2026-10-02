use std::collections::BTreeMap;

use crate::control::query::{ServiceTimerArming, ServiceTimerView};
use crate::timer::boot::TimerBootPlanError;

use super::entry::LinuxCalendarTimerEntry;

#[derive(Debug, Default)]
pub(in crate::runtime::linux) struct LinuxCalendarTimerTable {
    pub(super) entries: BTreeMap<i32, LinuxCalendarTimerEntry>,
    /// The triggers the last boot or reload refused to arm, as service,
    /// schedule and why, for `status` to say so: until now only the console
    /// heard of them.
    pub(super) rejected: Vec<(String, String, String)>,
}

impl LinuxCalendarTimerTable {
    pub(in crate::runtime::linux) fn new() -> Self {
        Self::default()
    }

    /// The service and schedule a timer fd belongs to.
    ///
    /// Needed to name a failed last-run write, which knows only the child's
    /// pid and the fd whose firing forked it (PEI-369).
    pub(in crate::runtime::linux) fn identity_for(&self, fd: i32) -> Option<(String, String)> {
        self.entries
            .get(&fd)
            .map(|entry| (entry.service.clone(), entry.schedule.clone()))
    }

    /// Every service's timers, armed or refused, each service's in the order
    /// of their schedules, so that a timer firing does not move it.
    pub(in crate::runtime::linux) fn views(&self) -> BTreeMap<String, Vec<ServiceTimerView>> {
        let mut views: BTreeMap<String, Vec<ServiceTimerView>> = BTreeMap::new();
        for entry in self.entries.values() {
            views.entry(entry.service.clone()).or_default().push(ServiceTimerView {
                schedule: entry.schedule.clone(),
                arming: ServiceTimerArming::Armed {
                    scheduled_ns: entry.next_scheduled_ns,
                    fires_ns: entry.armed_deadline_ns,
                    last_fired_ns: entry.last_fired_ns,
                },
            });
        }
        for (service, schedule, reason) in &self.rejected {
            views.entry(service.clone()).or_default().push(ServiceTimerView {
                schedule: schedule.clone(),
                arming: ServiceTimerArming::NotArmed {
                    reason: reason.clone(),
                },
            });
        }
        for timers in views.values_mut() {
            timers.sort_by(|a, b| a.schedule.cmp(&b.schedule));
        }
        views
    }
}

/// A refused trigger as the table keeps it: the service, the schedule, and
/// why, in the calendar's own words.
pub(super) fn rejected_trigger(error: &TimerBootPlanError) -> (String, String, String) {
    match error {
        TimerBootPlanError::Registry {
            service,
            schedule,
            source,
        } => (service.clone(), schedule.clone(), format!("{source:?}")),
        TimerBootPlanError::ParseSchedule {
            service,
            schedule,
            source,
        } => (service.clone(), schedule.clone(), source.to_string()),
        TimerBootPlanError::ComputeNext {
            service,
            schedule,
            source,
        } => (service.clone(), schedule.clone(), source.to_string()),
    }
}
