use super::super::{CalendarParseError, DayField, NumberField};

pub(super) struct DateFields {
    pub(super) years: NumberField,
    pub(super) months: NumberField,
    pub(super) days: DayField,
}

impl DateFields {
    pub(super) fn any() -> Self {
        Self {
            years: NumberField::any(0, 9999),
            months: NumberField::any(1, 12),
            days: DayField::any(),
        }
    }
}

pub(super) fn parse_date(expression: &str) -> Result<DateFields, CalendarParseError> {
    let Some((year, rest)) = expression.split_once('-') else {
        return Err(CalendarParseError::UnknownToken {
            token: expression.to_string(),
        });
    };
    let (month, day, last_day) = if let Some((month, day)) = rest.split_once('~') {
        (month.strip_suffix('-').unwrap_or(month), day, true)
    } else if let Some((month, day)) = rest.rsplit_once('-') {
        (month, day, false)
    } else {
        return Err(CalendarParseError::invalid_field(
            "date",
            expression,
            "date must contain year, month and day",
        ));
    };

    let day_expression = if last_day {
        format!("~{day}")
    } else {
        day.to_string()
    };

    Ok(DateFields {
        years: NumberField::parse(year, 0, 9999, "year")?,
        months: NumberField::parse(month, 1, 12, "month")?,
        days: DayField::parse(&day_expression)?,
    })
}
