#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::init::linux) struct RtcTime {
    pub sec: i32,
    pub min: i32,
    pub hour: i32,
    pub mday: i32,
    pub mon: i32,
    pub year: i32,
}

pub(super) fn rtc_time_to_unix_seconds(time: RtcTime) -> Result<i64, String> {
    validate_time_fields(time)?;
    let year = rtc_year(time.year)?;
    let month = time.mon + 1;
    let days_in_month = days_in_month(year, month);
    if !(1..=days_in_month).contains(&time.mday) {
        return Err(format!(
            "day {} out of range for {year}-{month:02}",
            time.mday
        ));
    }

    let days = days_from_civil(year, month, time.mday);
    let seconds = days
        .checked_mul(86_400)
        .and_then(|value| value.checked_add(i64::from(time.hour) * 3_600))
        .and_then(|value| value.checked_add(i64::from(time.min) * 60))
        .and_then(|value| value.checked_add(i64::from(time.sec)))
        .ok_or_else(|| "timestamp overflows i64".to_string())?;
    if seconds < 0 {
        Err(format!("timestamp {seconds} is before Unix epoch"))
    } else {
        Ok(seconds)
    }
}

fn validate_time_fields(time: RtcTime) -> Result<(), String> {
    if !(0..=59).contains(&time.sec) {
        return Err(format!("seconds out of range: {}", time.sec));
    }
    if !(0..=59).contains(&time.min) {
        return Err(format!("minutes out of range: {}", time.min));
    }
    if !(0..=23).contains(&time.hour) {
        return Err(format!("hours out of range: {}", time.hour));
    }
    if !(0..=11).contains(&time.mon) {
        return Err(format!("month out of range: {}", time.mon));
    }
    Ok(())
}

fn rtc_year(year_since_1900: i32) -> Result<i32, String> {
    let year = 1900_i32
        .checked_add(year_since_1900)
        .ok_or_else(|| format!("year overflows: {year_since_1900}"))?;
    if year < 1970 {
        return Err(format!("year {year} is before 1970"));
    }
    Ok(year)
}

fn days_from_civil(year: i32, month: i32, day: i32) -> i64 {
    let mut y = i64::from(year);
    let m = i64::from(month);
    let d = i64::from(day);
    y -= i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
