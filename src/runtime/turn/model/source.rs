#[cfg(feature = "peios-boundary")]
use crate::boundary::LinuxPowerButtonDevices;
use crate::boundary::{
    LinuxPid1SignalFd, LinuxPowerButtonRead, LinuxPowerButtonReadError, LinuxSignalFdRead,
    LinuxSignalFdReadError, LinuxTimerFd, LinuxTimerFdRead, LinuxTimerFdReadError,
    ShutdownDeadlineTimer,
};

pub trait RuntimePid1SignalSource {
    fn read_pid1_signal(&mut self) -> Result<LinuxSignalFdRead, LinuxSignalFdReadError>;
}

impl RuntimePid1SignalSource for LinuxPid1SignalFd {
    fn read_pid1_signal(&mut self) -> Result<LinuxSignalFdRead, LinuxSignalFdReadError> {
        self.read_signal()
    }
}

pub trait RuntimeShutdownDeadlineTimer: ShutdownDeadlineTimer {
    fn read_shutdown_deadline(&mut self) -> Result<LinuxTimerFdRead, LinuxTimerFdReadError>;
}

impl RuntimeShutdownDeadlineTimer for LinuxTimerFd {
    fn read_shutdown_deadline(&mut self) -> Result<LinuxTimerFdRead, LinuxTimerFdReadError> {
        self.read_expirations()
    }
}

pub trait RuntimeLifecycleDeadlineTimer: ShutdownDeadlineTimer {
    fn read_lifecycle_deadline(&mut self) -> Result<LinuxTimerFdRead, LinuxTimerFdReadError>;
}

impl RuntimeLifecycleDeadlineTimer for LinuxTimerFd {
    fn read_lifecycle_deadline(&mut self) -> Result<LinuxTimerFdRead, LinuxTimerFdReadError> {
        self.read_expirations()
    }
}

pub trait RuntimePowerButtonSource {
    fn read_power_button(
        &mut self,
        fd: i32,
    ) -> Result<LinuxPowerButtonRead, LinuxPowerButtonReadError>;
}

#[cfg(feature = "peios-boundary")]
impl RuntimePowerButtonSource for LinuxPowerButtonDevices {
    fn read_power_button(
        &mut self,
        fd: i32,
    ) -> Result<LinuxPowerButtonRead, LinuxPowerButtonReadError> {
        self.read_power_button(fd)
    }
}
