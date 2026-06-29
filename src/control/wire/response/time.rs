#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlResponseTimeProjection {
    pub monotonic_now_ns: u64,
    pub realtime_now_ns: u64,
}

impl ControlResponseTimeProjection {
    pub const fn new(monotonic_now_ns: u64, realtime_now_ns: u64) -> Self {
        Self {
            monotonic_now_ns,
            realtime_now_ns,
        }
    }

    pub(super) fn realtime_timestamp(self, monotonic_event_ns: u64) -> String {
        rfc3339_from_unix_ns(self.project_unix_ns(monotonic_event_ns))
    }

    pub(super) fn uptime_seconds(self, started_at_ns: Option<u64>) -> Option<u64> {
        started_at_ns.map(|started_at_ns| {
            self.monotonic_now_ns.saturating_sub(started_at_ns) / 1_000_000_000
        })
    }

    fn project_unix_ns(self, monotonic_event_ns: u64) -> u64 {
        if monotonic_event_ns <= self.monotonic_now_ns {
            self.realtime_now_ns
                .saturating_sub(self.monotonic_now_ns - monotonic_event_ns)
        } else {
            self.realtime_now_ns
                .saturating_add(monotonic_event_ns - self.monotonic_now_ns)
        }
    }
}

fn rfc3339_from_unix_ns(ns: u64) -> String {
    let seconds = ns / 1_000_000_000;
    let nanos = ns % 1_000_000_000;
    let days = seconds / 86_400;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{nanos:09}Z")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    (year, month as u32, day as u32)
}
