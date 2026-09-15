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
    // Split before looking for a fraction: `..` is the range operator, and a
    // check over the whole token read `8..17:00` as fractional seconds
    // (PEI-845). A fraction is a dot inside the seconds field, and the range
    // operator is two dots, so the seconds field is what gets checked and a
    // lone dot is what it looks for.
    let parts: Vec<&str> = expression.split(':').collect();
    match parts.as_slice() {
        [hour, minute] => Ok(TimeFields {
            hours: NumberField::parse(hour, 0, 23, "hour")?,
            minutes: NumberField::parse(minute, 0, 59, "minute")?,
            seconds: NumberField::parse("0", 0, 59, "second")?,
        }),
        [hour, minute, second] => {
            reject_fractional_seconds(second, expression)?;
            Ok(TimeFields {
                hours: NumberField::parse(hour, 0, 23, "hour")?,
                minutes: NumberField::parse(minute, 0, 59, "minute")?,
                seconds: NumberField::parse(second, 0, 59, "second")?,
            })
        }
        _ => Err(CalendarParseError::invalid_field(
            "time",
            expression,
            "time must contain hour:minute or hour:minute:second",
        )),
    }
}

/// A fraction is a single dot with digits after it, as in `15.5`. A field
/// containing `..` is a range, which is the number parser's business.
fn reject_fractional_seconds(second: &str, expression: &str) -> Result<(), CalendarParseError> {
    let has_fraction = second
        .split("..")
        .any(|number| number.split('/').any(|part| part.contains('.')));
    if has_fraction {
        return Err(CalendarParseError::FractionalSecondsUnsupported {
            value: expression.to_string(),
        });
    }
    Ok(())
}
