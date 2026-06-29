use std::collections::BTreeSet;

use jiff::civil::Weekday;

use super::super::CalendarParseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::timer::calendar) struct WeekdayField {
    values: BTreeSet<u8>,
}

impl WeekdayField {
    pub(in crate::timer::calendar) fn any() -> Self {
        Self {
            values: (1..=7).collect(),
        }
    }

    pub(in crate::timer::calendar) fn parse(expression: &str) -> Result<Self, CalendarParseError> {
        if expression.is_empty() {
            return Err(CalendarParseError::invalid_field(
                "weekday",
                expression,
                "field is empty",
            ));
        }

        let mut values = BTreeSet::new();
        for part in expression.split(',') {
            if part.is_empty() {
                return Err(CalendarParseError::invalid_field(
                    "weekday",
                    expression,
                    "list contains an empty item",
                ));
            }
            insert_weekday_part(part, expression, &mut values)?;
        }

        Ok(Self { values })
    }

    pub(in crate::timer::calendar) fn matches(&self, weekday: Weekday) -> bool {
        self.values
            .contains(&(weekday.to_monday_one_offset() as u8))
    }
}

fn insert_weekday_part(
    part: &str,
    expression: &str,
    values: &mut BTreeSet<u8>,
) -> Result<(), CalendarParseError> {
    let (base, step) = split_step(part, "weekday", expression)?;
    let (start, end, single) = if base == "*" {
        (1, 7, false)
    } else if let Some((start, end)) = base.split_once("..") {
        let start = parse_weekday_name(start, expression)?;
        let end = parse_weekday_name(end, expression)?;
        if start > end {
            return Err(CalendarParseError::invalid_field(
                "weekday",
                expression,
                "range start is after range end",
            ));
        }
        (start, end, false)
    } else {
        let start = parse_weekday_name(base, expression)?;
        (start, start, true)
    };

    let end = if single && step.is_some() { 7 } else { end };
    insert_ascending_weekdays(start, end, step.unwrap_or(1), values);
    Ok(())
}

fn split_step<'a>(
    expression: &'a str,
    field: &'static str,
    full_expression: &str,
) -> Result<(&'a str, Option<u16>), CalendarParseError> {
    let Some((base, step)) = expression.split_once('/') else {
        return Ok((expression, None));
    };
    if base.is_empty() || step.is_empty() || step.contains('/') {
        return Err(CalendarParseError::invalid_field(
            field,
            full_expression,
            "invalid repetition syntax",
        ));
    }
    let step = step
        .parse::<u16>()
        .map_err(|_| CalendarParseError::invalid_field(field, full_expression, "invalid step"))?;
    if step == 0 {
        return Err(CalendarParseError::invalid_field(
            field,
            full_expression,
            "step must be greater than zero",
        ));
    }
    Ok((base, Some(step)))
}

fn parse_weekday_name(expression: &str, full_expression: &str) -> Result<u8, CalendarParseError> {
    let value = match expression.to_ascii_lowercase().as_str() {
        "mon" | "monday" => 1,
        "tue" | "tues" | "tuesday" => 2,
        "wed" | "wednesday" => 3,
        "thu" | "thur" | "thurs" | "thursday" => 4,
        "fri" | "friday" => 5,
        "sat" | "saturday" => 6,
        "sun" | "sunday" => 7,
        _ => {
            return Err(CalendarParseError::invalid_field(
                "weekday",
                full_expression,
                "unknown weekday name",
            ));
        }
    };
    Ok(value)
}

fn insert_ascending_weekdays(start: u8, end: u8, step: u16, values: &mut BTreeSet<u8>) {
    let mut value = u16::from(start);
    let end = u16::from(end);
    while value <= end {
        values.insert(value as u8);
        match value.checked_add(step) {
            Some(next) => value = next,
            None => break,
        }
    }
}
