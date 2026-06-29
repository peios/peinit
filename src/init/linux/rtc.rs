mod syscalls;
#[cfg(test)]
mod tests;
mod time;

use std::io;
use std::path::Path;

use crate::boundary::BoundaryError;

pub(super) use syscalls::{LinuxRtcClockSyscalls, RtcClockSyscalls};
pub(super) use time::RtcTime;

use time::rtc_time_to_unix_seconds;

const PRIMARY_RTC_PATH: &str = "/dev/rtc";
const FALLBACK_RTC_PATH: &str = "/dev/rtc0";

pub(super) fn set_clock_from_hardware_rtc<S>(syscalls: &mut S) -> Result<(), BoundaryError>
where
    S: RtcClockSyscalls + ?Sized,
{
    let fd = open_rtc_fd(syscalls)?;
    let rtc_time = match syscalls.read_rtc_time(fd) {
        Ok(rtc_time) => rtc_time,
        Err(error) => {
            let _ = syscalls.close_fd(fd);
            return Err(BoundaryError::Recovery(format!(
                "read RTC time from fd {fd} failed: {error}",
            )));
        }
    };
    syscalls.close_fd(fd).map_err(|error| {
        BoundaryError::Recovery(format!(
            "close RTC fd {fd} after successful read failed: {error}"
        ))
    })?;
    let seconds = rtc_time_to_unix_seconds(rtc_time).map_err(|error| {
        BoundaryError::Recovery(format!("RTC value is invalid or pre-Unix-epoch: {error}"))
    })?;
    syscalls
        .set_realtime(seconds, 0)
        .map_err(|error| BoundaryError::Recovery(format!("clock_settime failed: {error}")))
}

fn open_rtc_fd<S>(syscalls: &mut S) -> Result<i32, BoundaryError>
where
    S: RtcClockSyscalls + ?Sized,
{
    match syscalls.open_rtc_device(Path::new(PRIMARY_RTC_PATH)) {
        Ok(fd) => Ok(fd),
        Err(error) if device_absent(&error) => syscalls
            .open_rtc_device(Path::new(FALLBACK_RTC_PATH))
            .map_err(|fallback| {
                BoundaryError::Recovery(format!(
                    "open {PRIMARY_RTC_PATH} failed because it is absent, and open {FALLBACK_RTC_PATH} failed: {fallback}",
                ))
            }),
        Err(error) => Err(BoundaryError::Recovery(format!(
            "open {PRIMARY_RTC_PATH} failed: {error}"
        ))),
    }
}

fn device_absent(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(errno) if errno == libc::ENOENT || errno == libc::ENODEV
    )
}
