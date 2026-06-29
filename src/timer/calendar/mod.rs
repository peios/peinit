mod eval;
mod field;
mod parse;

#[cfg(test)]
mod tests;

use std::fmt;

use field::{DayField, NumberField, WeekdayField};

pub use parse::parse_calendar_schedule;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarSchedule {
    weekdays: WeekdayField,
    years: NumberField,
    months: NumberField,
    days: DayField,
    hours: NumberField,
    minutes: NumberField,
    seconds: NumberField,
    timezone: Option<String>,
}

impl CalendarSchedule {
    pub fn parse(expression: &str) -> Result<Self, CalendarParseError> {
        parse_calendar_schedule(expression)
    }

    pub fn next_after_ns(&self, after_realtime_ns: u64) -> Result<u64, CalendarNextError> {
        eval::next_after_ns(self, after_realtime_ns)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarParseError {
    Empty,
    MissingSchedule,
    UnknownToken {
        token: String,
    },
    TooManyParts {
        expression: String,
    },
    InvalidField {
        field: &'static str,
        value: String,
        reason: &'static str,
    },
    FractionalSecondsUnsupported {
        value: String,
    },
    InvalidTimezone {
        timezone: String,
        message: String,
    },
}

impl CalendarParseError {
    pub(super) fn invalid_field(
        field: &'static str,
        value: impl Into<String>,
        reason: &'static str,
    ) -> Self {
        Self::InvalidField {
            field,
            value: value.into(),
            reason,
        }
    }
}

impl fmt::Display for CalendarParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "calendar expression is empty"),
            Self::MissingSchedule => write!(f, "calendar expression has no schedule component"),
            Self::UnknownToken { token } => write!(f, "unknown calendar token '{token}'"),
            Self::TooManyParts { expression } => {
                write!(f, "calendar expression has too many parts: '{expression}'")
            }
            Self::InvalidField {
                field,
                value,
                reason,
            } => write!(f, "invalid {field} field '{value}': {reason}"),
            Self::FractionalSecondsUnsupported { value } => {
                write!(f, "fractional seconds are not supported in '{value}'")
            }
            Self::InvalidTimezone { timezone, message } => {
                write!(f, "invalid timezone '{timezone}': {message}")
            }
        }
    }
}

impl std::error::Error for CalendarParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarNextError {
    TimestampOutOfRange { value: u64, message: String },
    CivilTimeOutOfRange { message: String },
    TimezoneUnavailable { timezone: String, message: String },
    NoFutureOccurrence,
}

impl fmt::Display for CalendarNextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimestampOutOfRange { value, message } => {
                write!(
                    f,
                    "timestamp {value}ns is outside calendar range: {message}"
                )
            }
            Self::CivilTimeOutOfRange { message } => {
                write!(f, "civil time is outside calendar range: {message}")
            }
            Self::TimezoneUnavailable { timezone, message } => {
                write!(f, "timezone '{timezone}' is unavailable: {message}")
            }
            Self::NoFutureOccurrence => write!(f, "calendar expression has no future occurrence"),
        }
    }
}

impl std::error::Error for CalendarNextError {}
