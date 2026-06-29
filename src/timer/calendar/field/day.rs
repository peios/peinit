use std::collections::BTreeSet;

use jiff::civil::Date;

use super::super::CalendarParseError;
use super::number::{NumberField, parse_values};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::timer::calendar) enum DayField {
    MonthDay(NumberField),
    LastDay { offsets: BTreeSet<u16> },
}

impl DayField {
    pub(in crate::timer::calendar) fn any() -> Self {
        Self::MonthDay(NumberField::any(1, 31))
    }

    pub(in crate::timer::calendar) fn parse(expression: &str) -> Result<Self, CalendarParseError> {
        if let Some(offsets) = expression.strip_prefix('~') {
            if offsets.is_empty() {
                return Err(CalendarParseError::invalid_field(
                    "day",
                    expression,
                    "last-day offset is empty",
                ));
            }
            return Ok(Self::LastDay {
                offsets: parse_values(offsets, 1, 31, "day", true)?,
            });
        }
        NumberField::parse(expression, 1, 31, "day").map(Self::MonthDay)
    }

    pub(in crate::timer::calendar) fn matches(&self, date: Date) -> bool {
        let day = date.day() as u16;
        match self {
            Self::MonthDay(days) => days.contains(day),
            Self::LastDay { offsets } => {
                let offset_from_last = date.days_in_month() as u16 - day + 1;
                offsets.contains(&offset_from_last)
            }
        }
    }
}
