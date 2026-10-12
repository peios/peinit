use std::collections::{BTreeMap, BTreeSet};

use crate::boundary::write_linux_console_message;
use crate::boundary::{Clock, RealtimeClock};
use crate::runtime::{RuntimeEventRegistrar, RuntimeEventSource};
use crate::supervisor::Supervisor;
use crate::timer::boot::plan_timer_reload_where;

use super::entry::LinuxCalendarTimerEntry;
use super::error::LinuxCalendarTimerError;
use super::registration::{definitions_from_supervisor, register_calendar_timer};
use super::table::{LinuxCalendarTimerTable, rejected_trigger};

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
        let mut retained = BTreeSet::new();
        let plan = plan_timer_reload_where(
            &definitions,
            realtime_now_ns,
            |service, schedule, storage| {
                // Match each existing fd at most once, including duplicate
                // schedules. Bookkeeping writes and unrelated service edits
                // must not redraw jitter or postpone an already-armed firing.
                let existing = self.entries.iter().find(|(fd, entry)| {
                    !retained.contains(*fd)
                        && entry.service == service.name
                        && entry.schedule == schedule
                        && entry.storage == storage
                        && entry.persistent == service.timer_persistent
                        && entry.jitter_secs == service.timer_jitter_secs
                });
                if let Some((&fd, _)) = existing {
                    retained.insert(fd);
                    false
                } else {
                    true
                }
            },
        )
        .map_err(LinuxCalendarTimerError::BootPlan)?;
        // A bad schedule introduced by an edit used to come out of the run
        // loop and end PID 1's event loop -- and any drained registry watch
        // event triggers a reload, so writing the schedule was enough, without
        // anyone running `reload-config`. It now costs that one trigger.
        for error in &plan.rejected {
            let _ = write_linux_console_message(&format!(
                "peinit warning: calendar timer not armed after reload: {error:?}\n"
            ));
        }
        let mut replacement = BTreeMap::new();
        let mut sources = Vec::new();
        // Changed settings re-arm from now without reading history, but the
        // last firing still belongs to the same service and schedule.
        let fired = self
            .entries
            .values()
            .filter_map(|entry| {
                entry
                    .last_fired_ns
                    .map(|ns| ((entry.service.clone(), entry.schedule.clone()), ns))
            })
            .collect::<BTreeMap<_, _>>();

        for registration in plan.registrations {
            match register_calendar_timer(registration, registrar) {
                Ok((source, mut entry)) => {
                    entry.last_fired_ns = fired
                        .get(&(entry.service.clone(), entry.schedule.clone()))
                        .copied();
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
            if retained.contains(&fd) {
                continue;
            }
            if let Err(error) = registrar.unregister_source(fd) {
                unregister_replacement_sources(&replacement, registrar);
                return Err(LinuxCalendarTimerError::Register(error));
            }
        }

        for fd in retained {
            sources.push(RuntimeEventSource::CalendarTimer { fd });
            replacement.insert(fd, self.entries.remove(&fd).expect("retained timer"));
        }
        self.entries = replacement;
        self.rejected = plan.rejected.iter().map(rejected_trigger).collect();
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

#[cfg(test)]
mod tests;
