use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::{AmbiguousOffset, TimeZone};

use super::{CalendarNextError, CalendarSchedule};

pub(super) fn next_after_ns(
    schedule: &CalendarSchedule,
    after_realtime_ns: u64,
) -> Result<u64, CalendarNextError> {
    let after = Timestamp::from_nanosecond(i128::from(after_realtime_ns)).map_err(|err| {
        CalendarNextError::TimestampOutOfRange {
            value: after_realtime_ns,
            message: err.to_string(),
        }
    })?;
    let timezone = resolve_timezone(schedule)?;
    let mut date = after.to_zoned(timezone.clone()).date();
    let times = scheduled_seconds_of_day(schedule);

    loop {
        if date_matches(schedule, date) {
            for second_of_day in &times {
                let candidate = candidate_ns(date, *second_of_day, &timezone)?;
                if let Some(candidate_ns) = candidate.filter(|ns| *ns > after_realtime_ns) {
                    return Ok(candidate_ns);
                }
            }
        }

        date = date
            .tomorrow()
            .map_err(|_| CalendarNextError::NoFutureOccurrence)?;
    }
}

fn resolve_timezone(schedule: &CalendarSchedule) -> Result<TimeZone, CalendarNextError> {
    match &schedule.timezone {
        Some(timezone) => {
            TimeZone::get(timezone).map_err(|err| CalendarNextError::TimezoneUnavailable {
                timezone: timezone.clone(),
                message: err.to_string(),
            })
        }
        None => Ok(TimeZone::system()),
    }
}

fn scheduled_seconds_of_day(schedule: &CalendarSchedule) -> Vec<u32> {
    let mut seconds = Vec::new();
    for hour in schedule.hours.values() {
        for minute in schedule.minutes.values() {
            for second in schedule.seconds.values() {
                seconds.push(u32::from(hour) * 3_600 + u32::from(minute) * 60 + u32::from(second));
            }
        }
    }
    seconds
}

fn date_matches(schedule: &CalendarSchedule, date: Date) -> bool {
    schedule.years.contains(date.year() as u16)
        && schedule.months.contains(date.month() as u16)
        && schedule.days.matches(date)
        && schedule.weekdays.matches(date.weekday())
}

fn candidate_ns(
    date: Date,
    second_of_day: u32,
    timezone: &TimeZone,
) -> Result<Option<u64>, CalendarNextError> {
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    let datetime = DateTime::new(
        date.year(),
        date.month(),
        date.day(),
        hour as i8,
        minute as i8,
        second as i8,
        0,
    )
    .map_err(|err| CalendarNextError::CivilTimeOutOfRange {
        message: err.to_string(),
    })?;

    let ambiguous = timezone.to_ambiguous_zoned(datetime);
    if matches!(ambiguous.offset(), AmbiguousOffset::Gap { .. }) {
        return Ok(None);
    }

    let candidate = ambiguous
        .earlier()
        .map_err(|err| CalendarNextError::CivilTimeOutOfRange {
            message: err.to_string(),
        })?;
    let nanos = candidate.timestamp().as_nanosecond();
    let nanos = u64::try_from(nanos).map_err(|_| CalendarNextError::CivilTimeOutOfRange {
        message: format!("candidate timestamp {nanos}ns is before the Unix epoch"),
    })?;
    Ok(Some(nanos))
}
