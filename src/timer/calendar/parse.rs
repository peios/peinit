mod date;
mod shortcut;
mod time;
mod timezone;

use super::{CalendarParseError, CalendarSchedule, WeekdayField};
use date::{DateFields, parse_date};
use shortcut::expand_shortcut;
use time::{TimeFields, parse_time};
use timezone::validate_timezone;

pub fn parse_calendar_schedule(expression: &str) -> Result<CalendarSchedule, CalendarParseError> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err(CalendarParseError::Empty);
    }

    let (expression, from_shortcut) = expand_shortcut(expression);
    let tokens: Vec<&str> = expression.split_whitespace().collect();
    if tokens.is_empty() {
        return Err(CalendarParseError::Empty);
    }

    let mut cursor = 0;
    let mut saw_schedule_component = from_shortcut;

    let weekdays = if looks_like_weekday(tokens[cursor]) {
        cursor += 1;
        saw_schedule_component = true;
        WeekdayField::parse(tokens[cursor - 1])?
    } else {
        WeekdayField::any()
    };

    let days = if cursor < tokens.len() && looks_like_date(tokens[cursor]) {
        cursor += 1;
        saw_schedule_component = true;
        parse_date(tokens[cursor - 1])?
    } else {
        DateFields::any()
    };

    let time = if cursor < tokens.len() && looks_like_time(tokens[cursor]) {
        cursor += 1;
        saw_schedule_component = true;
        parse_time(tokens[cursor - 1])?
    } else {
        TimeFields::midnight()?
    };

    let timezone = if cursor < tokens.len() {
        let timezone = tokens[cursor];
        cursor += 1;
        validate_timezone(timezone)?;
        Some(timezone.to_string())
    } else {
        None
    };

    if cursor < tokens.len() {
        return Err(CalendarParseError::TooManyParts {
            expression: expression.to_string(),
        });
    }
    if !saw_schedule_component {
        return Err(CalendarParseError::MissingSchedule);
    }

    Ok(CalendarSchedule {
        weekdays,
        years: days.years,
        months: days.months,
        days: days.days,
        hours: time.hours,
        minutes: time.minutes,
        seconds: time.seconds,
        timezone,
    })
}

fn looks_like_weekday(token: &str) -> bool {
    token == "*" || WeekdayField::parse(token).is_ok()
}

fn looks_like_date(token: &str) -> bool {
    token.contains('-') || token.contains('~')
}

fn looks_like_time(token: &str) -> bool {
    token.contains(':')
        && token
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit() || ch == '*')
}
