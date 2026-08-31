use std::collections::BTreeMap;

use super::entry::LinuxCalendarTimerEntry;

#[derive(Debug, Default)]
pub(in crate::runtime::linux) struct LinuxCalendarTimerTable {
    pub(super) entries: BTreeMap<i32, LinuxCalendarTimerEntry>,
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
}
