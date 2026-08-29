//! The `PROGRESS=` grammar (PSPU §4.19): `N`, `N/`, or `N/T`.

use super::model::{JobProgress, JobProgressUnit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressParseError {
    /// Not one of the three forms, or a number that does not parse.
    Malformed,
    /// `T` was zero.
    ZeroTotal,
    /// `N` exceeded `T`.
    CurrentExceedsTotal,
}

/// Parse a `PROGRESS=` value. A value that fails is an *unexpected value*
/// under §4.17: the line is ignored, never clamped or repaired.
pub fn parse_progress(value: &str) -> Result<JobProgress, ProgressParseError> {
    let (current, rest) = match value.split_once('/') {
        Some((current, rest)) => (current, Some(rest)),
        None => (value, None),
    };
    let current = parse_unsigned(current)?;
    match rest {
        None => Ok(JobProgress {
            current,
            total: None,
            bounded: false,
        }),
        Some("") => Ok(JobProgress {
            current,
            total: None,
            bounded: true,
        }),
        Some(total) => {
            let total = parse_unsigned(total)?;
            if total == 0 {
                return Err(ProgressParseError::ZeroTotal);
            }
            if current > total {
                return Err(ProgressParseError::CurrentExceedsTotal);
            }
            Ok(JobProgress {
                current,
                total: Some(total),
                bounded: true,
            })
        }
    }
}

/// Parse a `PROGRESS_UNIT=` value; anything outside the vocabulary is an
/// unexpected value and is ignored.
pub fn parse_progress_unit(value: &str) -> Option<JobProgressUnit> {
    match value {
        "bytes" => Some(JobProgressUnit::Bytes),
        "items" => Some(JobProgressUnit::Items),
        "percent" => Some(JobProgressUnit::Percent),
        _ => None,
    }
}

/// Decimal digits only: no sign, no whitespace, no prefix. `str::parse`
/// accepts a leading `+`, which the grammar does not.
fn parse_unsigned(text: &str) -> Result<u64, ProgressParseError> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProgressParseError::Malformed);
    }
    text.parse().map_err(|_| ProgressParseError::Malformed)
}
