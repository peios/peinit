use super::super::{CalendarParseError, NumberField};

pub(super) struct TimeFields {
    pub(super) hours: NumberField,
    pub(super) minutes: NumberField,
    pub(super) seconds: NumberField,
}

impl TimeFields {
    pub(super) fn midnight() -> Result<Self, CalendarParseError> {
        Ok(Self {
            hours: NumberField::parse("0", 0, 23, "hour")?,
            minutes: NumberField::parse("0", 0, 59, "minute")?,
            seconds: NumberField::parse("0", 0, 59, "second")?,
        })
    }
}

pub(super) fn parse_time(expression: &str) -> Result<TimeFields, CalendarParseError> {
    if expression.contains('.') {
        return Err(CalendarParseError::FractionalSecondsUnsupported {
            value: expression.to_string(),
        });
    }

    let parts: Vec<&str> = expression.split(':').collect();
    match parts.as_slice() {
        [hour, minute] => Ok(TimeFields {
            hours: NumberField::parse(hour, 0, 23, "hour")?,
            minutes: NumberField::parse(minute, 0, 59, "minute")?,
            seconds: NumberField::parse("0", 0, 59, "second")?,
        }),
        [hour, minute, second] => Ok(TimeFields {
            hours: NumberField::parse(hour, 0, 23, "hour")?,
            minutes: NumberField::parse(minute, 0, 59, "minute")?,
            seconds: NumberField::parse(second, 0, 59, "second")?,
        }),
        _ => Err(CalendarParseError::invalid_field(
            "time",
            expression,
            "time must contain hour:minute or hour:minute:second",
        )),
    }
}
