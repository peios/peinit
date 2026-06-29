use crate::boundary::{
    LinuxPid1SignalFd, LinuxSignalFdRead, LinuxSignalFdReadError, LinuxTimerFd, LinuxTimerFdRead,
    LinuxTimerFdReadError, ShutdownDeadlineTimer,
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
