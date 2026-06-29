use jiff::tz::TimeZone;

use super::super::CalendarParseError;

pub(super) fn validate_timezone(timezone: &str) -> Result<(), CalendarParseError> {
    TimeZone::get(timezone)
        .map(|_| ())
        .map_err(|err| CalendarParseError::InvalidTimezone {
            timezone: timezone.to_string(),
            message: err.to_string(),
        })
}
