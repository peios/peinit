use std::collections::BTreeSet;

use super::super::CalendarParseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::timer::calendar) struct NumberField {
    values: BTreeSet<u16>,
}

impl NumberField {
    pub(in crate::timer::calendar) fn any(min: u16, max: u16) -> Self {
        Self {
            values: (min..=max).collect(),
        }
    }

    pub(in crate::timer::calendar) fn parse(
        expression: &str,
        min: u16,
        max: u16,
        field: &'static str,
    ) -> Result<Self, CalendarParseError> {
        let values = parse_values(expression, min, max, field, false)?;
        Ok(Self { values })
    }

    pub(in crate::timer::calendar) fn contains(&self, value: u16) -> bool {
        self.values.contains(&value)
    }

    pub(in crate::timer::calendar) fn values(&self) -> impl Iterator<Item = u16> + '_ {
        self.values.iter().copied()
    }
}

pub(in crate::timer::calendar::field) fn parse_values(
    expression: &str,
    min: u16,
    max: u16,
    field: &'static str,
    last_day_offsets: bool,
) -> Result<BTreeSet<u16>, CalendarParseError> {
    if expression.is_empty() {
        return Err(CalendarParseError::invalid_field(
            field,
            expression,
            "field is empty",
        ));
    }

    let mut values = BTreeSet::new();
    for part in expression.split(',') {
        if part.is_empty() {
            return Err(CalendarParseError::invalid_field(
                field,
                expression,
                "list contains an empty item",
            ));
        }
        insert_numeric_part(
            part,
            expression,
            min,
            max,
            field,
            last_day_offsets,
            &mut values,
        )?;
    }
    Ok(values)
}

fn insert_numeric_part(
    part: &str,
    expression: &str,
    min: u16,
    max: u16,
    field: &'static str,
    last_day_offsets: bool,
    values: &mut BTreeSet<u16>,
) -> Result<(), CalendarParseError> {
    let (base, step) = split_step(part, field, expression)?;
    let has_step = step.is_some();
    let (start, end, single) = if base == "*" {
        (min, max, false)
    } else if let Some((start, end)) = base.split_once("..") {
        let start = parse_number(start, expression, min, max, field)?;
        let end = parse_number(end, expression, min, max, field)?;
        if start > end {
            return Err(CalendarParseError::invalid_field(
                field,
                expression,
                "range start is after range end",
            ));
        }
        (start, end, false)
    } else {
        let start = parse_number(base, expression, min, max, field)?;
        (start, start, true)
    };

    let step = step.unwrap_or(1);
    if last_day_offsets && single && has_step {
        insert_descending(start, min, step, values);
    } else {
        let end = if single && has_step { max } else { end };
        insert_ascending(start, end, step, values);
    }
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

fn parse_number(
    expression: &str,
    full_expression: &str,
    min: u16,
    max: u16,
    field: &'static str,
) -> Result<u16, CalendarParseError> {
    if expression.is_empty() {
        return Err(CalendarParseError::invalid_field(
            field,
            full_expression,
            "number is empty",
        ));
    }
    let value = expression
        .parse::<u16>()
        .map_err(|_| CalendarParseError::invalid_field(field, full_expression, "invalid number"))?;
    if value < min || value > max {
        return Err(CalendarParseError::invalid_field(
            field,
            full_expression,
            "number is out of range",
        ));
    }
    Ok(value)
}

fn insert_ascending(start: u16, end: u16, step: u16, values: &mut BTreeSet<u16>) {
    let mut value = start;
    while value <= end {
        values.insert(value);
        match value.checked_add(step) {
            Some(next) => value = next,
            None => break,
        }
    }
}

fn insert_descending(start: u16, end: u16, step: u16, values: &mut BTreeSet<u16>) {
    let mut value = start;
    while value >= end {
        values.insert(value);
        if value < end.saturating_add(step) {
            break;
        }
        value -= step;
    }
}
