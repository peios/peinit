use jiff::civil::DateTime;
use jiff::tz::TimeZone;

use super::{CalendarParseError, CalendarSchedule};

#[test]
fn daily_shortcut_runs_at_next_midnight_utc() {
    let schedule = CalendarSchedule::parse("daily UTC").expect("schedule");
    assert_eq!(
        schedule
            .next_after_ns(ns_utc(2024, 5, 1, 12, 0, 0))
            .expect("next"),
        ns_utc(2024, 5, 2, 0, 0, 0),
    );
}

#[test]
fn time_only_schedule_matches_every_date() {
    let schedule = CalendarSchedule::parse("14:30 UTC").expect("schedule");
    assert_eq!(
        schedule
            .next_after_ns(ns_utc(2024, 5, 1, 14, 0, 0))
            .expect("next"),
        ns_utc(2024, 5, 1, 14, 30, 0),
    );
    assert_eq!(
        schedule
            .next_after_ns(ns_utc(2024, 5, 1, 14, 30, 0))
            .expect("next"),
        ns_utc(2024, 5, 2, 14, 30, 0),
    );
}

#[test]
fn weekday_ranges_are_inclusive() {
    let schedule = CalendarSchedule::parse("Mon..Fri *-*-* 09:00 UTC").expect("schedule");
    assert_eq!(
        schedule
            .next_after_ns(ns_utc(2024, 5, 3, 10, 0, 0))
            .expect("next"),
        ns_utc(2024, 5, 6, 9, 0, 0),
    );
}

#[test]
fn last_day_offsets_match_from_end_of_month() {
    let schedule = CalendarSchedule::parse("*-*-~01 00:00 UTC").expect("schedule");
    assert_eq!(
        schedule
            .next_after_ns(ns_utc(2024, 2, 27, 23, 0, 0))
            .expect("next"),
        ns_utc(2024, 2, 29, 0, 0, 0),
    );

    let schedule = CalendarSchedule::parse("Mon *-05~07/1 00:00 UTC").expect("schedule");
    assert_eq!(
        schedule
            .next_after_ns(ns_utc(2024, 5, 1, 0, 0, 0))
            .expect("next"),
        ns_utc(2024, 5, 27, 0, 0, 0),
    );
}

#[test]
fn spring_dst_gap_is_skipped() {
    let schedule = CalendarSchedule::parse("*-03-31 01:30 Europe/London").expect("schedule");

    assert_eq!(
        schedule
            .next_after_ns(ns_utc(2024, 3, 30, 0, 0, 0))
            .expect("next"),
        ns_utc(2025, 3, 31, 0, 30, 0),
    );
}

#[test]
fn autumn_dst_fold_uses_first_occurrence() {
    let schedule = CalendarSchedule::parse("*-10-27 01:30 Europe/London").expect("schedule");

    assert_eq!(
        schedule
            .next_after_ns(ns_utc(2024, 10, 26, 0, 0, 0))
            .expect("next"),
        ns_utc(2024, 10, 27, 0, 30, 0),
    );
}

#[test]
fn fractional_seconds_are_rejected() {
    assert!(matches!(
        CalendarSchedule::parse("*-*-* 12:00:00.5 UTC"),
        Err(CalendarParseError::FractionalSecondsUnsupported { .. })
    ));
}

#[test]
fn invalid_timezone_is_rejected() {
    assert!(matches!(
        CalendarSchedule::parse("daily Not/AZone"),
        Err(CalendarParseError::InvalidTimezone { .. })
    ));
}

fn ns_utc(year: i16, month: i8, day: i8, hour: i8, minute: i8, second: i8) -> u64 {
    let nanos = DateTime::new(year, month, day, hour, minute, second, 0)
        .expect("datetime")
        .to_zoned(TimeZone::UTC)
        .expect("zoned")
        .timestamp()
        .as_nanosecond();
    u64::try_from(nanos).expect("positive timestamp")
}
