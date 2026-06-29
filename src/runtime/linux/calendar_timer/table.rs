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
}
