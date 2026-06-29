use std::fmt;

#[cfg(feature = "peios-registry")]
use crate::boundary::LinuxTimerFdCreateError;
use crate::boundary::{BoundaryError, LinuxTimerFdArmError, LinuxTimerFdReadError};
#[cfg(feature = "peios-registry")]
use crate::runtime::{RuntimeEventRegistrationError, RuntimeEventSourceDecodeError};
use crate::supervisor::SupervisorError;
#[cfg(feature = "peios-registry")]
use crate::timer::boot::TimerBootPlanError;
use crate::timer::calendar::CalendarNextError;
use crate::timer::jitter::TimerJitterError;

#[derive(Debug)]
pub(crate) enum LinuxCalendarTimerError {
    #[cfg(feature = "peios-registry")]
    BootPlan(TimerBootPlanError),
    Clock(BoundaryError),
    #[cfg(feature = "peios-registry")]
    Create(LinuxTimerFdCreateError),
    Arm(LinuxTimerFdArmError),
    Read(LinuxTimerFdReadError),
    #[cfg(feature = "peios-registry")]
    Register(RuntimeEventRegistrationError),
    #[cfg(feature = "peios-registry")]
    Source(RuntimeEventSourceDecodeError),
    Supervisor(SupervisorError),
    UnknownTimer {
        fd: i32,
    },
    #[cfg(feature = "peios-registry")]
    Parse {
        service: String,
        schedule: String,
        source: crate::timer::calendar::CalendarParseError,
    },
    Next(CalendarNextError),
    Jitter(TimerJitterError),
}

impl fmt::Display for LinuxCalendarTimerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "peios-registry")]
            Self::BootPlan(source) => write!(f, "timer boot plan failed: {source:?}"),
            Self::Clock(source) => write!(f, "clock read failed: {source:?}"),
            #[cfg(feature = "peios-registry")]
            Self::Create(source) => write!(f, "timerfd creation failed: {source:?}"),
            Self::Arm(source) => write!(f, "timerfd arm failed: {source:?}"),
            Self::Read(source) => write!(f, "timerfd read failed: {source:?}"),
            #[cfg(feature = "peios-registry")]
            Self::Register(source) => write!(f, "timerfd event registration failed: {source:?}"),
            #[cfg(feature = "peios-registry")]
            Self::Source(source) => write!(f, "timerfd event source creation failed: {source:?}"),
            Self::Supervisor(source) => write!(f, "supervisor timer dispatch failed: {source:?}"),
            Self::UnknownTimer { fd } => write!(f, "unknown calendar timer fd {fd}"),
            #[cfg(feature = "peios-registry")]
            Self::Parse {
                service,
                schedule,
                source,
            } => write!(
                f,
                "calendar schedule parse failed for service {service}, schedule {schedule:?}: {source:?}",
            ),
            Self::Next(source) => write!(f, "next calendar occurrence failed: {source:?}"),
            Self::Jitter(source) => write!(f, "timer jitter computation failed: {source:?}"),
        }
    }
}

impl std::error::Error for LinuxCalendarTimerError {}
