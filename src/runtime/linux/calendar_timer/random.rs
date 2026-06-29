use std::{fmt, io};

use crate::boundary::BoundaryError;
use crate::timer::jitter::{TimerJitterRandom, apply_timer_jitter};

use super::error::LinuxCalendarTimerError;

pub(super) fn jittered_deadline_ns(
    scheduled_ns: u64,
    jitter_secs: u64,
) -> Result<u64, LinuxCalendarTimerError> {
    let mut random = LinuxTimerJitterRandom;
    let deadline = apply_timer_jitter(scheduled_ns, jitter_secs, &mut random)
        .map_err(LinuxCalendarTimerError::Jitter)?;
    Ok(deadline.armed_ns)
}

#[derive(Debug, Default)]
struct LinuxTimerJitterRandom;

impl TimerJitterRandom for LinuxTimerJitterRandom {
    fn random_u64(&mut self) -> Result<u64, BoundaryError> {
        linux_random_u64().map_err(|error| BoundaryError::Timer(error.to_string()))
    }
}

#[derive(Debug)]
enum LinuxTimerJitterRandomError {
    GetRandom(io::Error),
    ShortRead { bytes: usize },
}

impl fmt::Display for LinuxTimerJitterRandomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GetRandom(source) => write!(f, "getrandom failed: {source}"),
            Self::ShortRead { bytes } => {
                write!(f, "getrandom returned short read of {bytes} bytes")
            }
        }
    }
}

fn linux_random_u64() -> Result<u64, LinuxTimerJitterRandomError> {
    let mut bytes = [0_u8; std::mem::size_of::<u64>()];
    loop {
        let rc =
            unsafe { libc::getrandom(bytes.as_mut_ptr().cast::<libc::c_void>(), bytes.len(), 0) };
        if rc < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(LinuxTimerJitterRandomError::GetRandom(error));
        }
        if rc as usize == bytes.len() {
            return Ok(u64::from_ne_bytes(bytes));
        }
        return Err(LinuxTimerJitterRandomError::ShortRead { bytes: rc as usize });
    }
}
