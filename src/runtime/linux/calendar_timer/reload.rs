use std::collections::BTreeMap;

use crate::boundary::{Clock, RealtimeClock};
use crate::runtime::{RuntimeEventRegistrar, RuntimeEventSource};
use crate::supervisor::Supervisor;
use crate::timer::boot::plan_timer_reload;

use super::entry::LinuxCalendarTimerEntry;
use super::error::LinuxCalendarTimerError;
use super::registration::{definitions_from_supervisor, register_calendar_timer};
use super::table::LinuxCalendarTimerTable;

impl LinuxCalendarTimerTable {
    pub(in crate::runtime::linux) fn reconfigure_timers<C, E>(
        &mut self,
        supervisor: &Supervisor,
        clock: &mut C,
        registrar: &mut E,
    ) -> Result<Vec<RuntimeEventSource>, LinuxCalendarTimerError>
    where
        C: Clock + RealtimeClock + ?Sized,
        E: RuntimeEventRegistrar + ?Sized,
    {
        let definitions = definitions_from_supervisor(supervisor);
        let realtime_now_ns = clock
            .realtime_ns()
            .map_err(LinuxCalendarTimerError::Clock)?;
        let plan = plan_timer_reload(&definitions, realtime_now_ns)
            .map_err(LinuxCalendarTimerError::BootPlan)?;
        let mut replacement = BTreeMap::new();
        let mut sources = Vec::new();

        for registration in plan.registrations {
            match register_calendar_timer(registration, registrar) {
                Ok((source, entry)) => {
                    sources.push(source);
                    replacement.insert(entry.timer.as_raw_fd(), entry);
                }
                Err(error) => {
                    unregister_replacement_sources(&replacement, registrar);
                    return Err(error);
                }
            }
        }

        let old_fds = self.entries.keys().copied().collect::<Vec<_>>();
        for fd in old_fds {
            if let Err(error) = registrar.unregister_source(fd) {
                unregister_replacement_sources(&replacement, registrar);
                return Err(LinuxCalendarTimerError::Register(error));
            }
        }

        self.entries = replacement;
        Ok(sources)
    }
}

fn unregister_replacement_sources<E>(
    replacement: &BTreeMap<i32, LinuxCalendarTimerEntry>,
    registrar: &mut E,
) where
    E: RuntimeEventRegistrar + ?Sized,
{
    for fd in replacement.keys().copied().collect::<Vec<_>>() {
        let _ = registrar.unregister_source(fd);
    }
}
