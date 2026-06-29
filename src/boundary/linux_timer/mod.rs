use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use super::{BoundaryError, ShutdownDeadlineTimer};

mod model;
mod syscall;

pub use model::{
    LinuxTimerFdArmError, LinuxTimerFdCreateError, LinuxTimerFdRead, LinuxTimerFdReadError,
    LinuxTimerSpec, linux_timer_spec_from_deadline_ns,
};
pub use syscall::{
    LinuxTimerFdSyscallApi, create_linux_monotonic_timerfd, create_linux_realtime_timerfd,
    read_linux_timerfd, set_linux_timerfd_absolute, set_linux_timerfd_realtime_absolute,
};

#[cfg(test)]
pub(super) use model::{
    TIMERFD_ABSOLUTE_FLAGS, TIMERFD_CREATE_CLOCK, TIMERFD_CREATE_FLAGS,
    TIMERFD_REALTIME_ABSOLUTE_FLAGS, TIMERFD_REALTIME_CREATE_CLOCK,
};

#[derive(Debug)]
pub struct LinuxTimerFd {
    fd: OwnedFd,
}

impl LinuxTimerFd {
    pub fn create_monotonic() -> Result<Self, LinuxTimerFdCreateError> {
        let mut syscalls = syscall::LinuxTimerFdSyscalls;
        let fd = create_linux_monotonic_timerfd(&mut syscalls)?;
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }

    pub fn create_realtime() -> Result<Self, LinuxTimerFdCreateError> {
        let mut syscalls = syscall::LinuxTimerFdSyscalls;
        let fd = create_linux_realtime_timerfd(&mut syscalls)?;
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }

    pub fn as_raw_fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    pub fn arm_absolute_ns(&self, deadline_ns: u64) -> Result<(), LinuxTimerFdArmError> {
        let mut syscalls = syscall::LinuxTimerFdSyscalls;
        set_linux_timerfd_absolute(&mut syscalls, self.fd.as_raw_fd(), deadline_ns)
    }

    pub fn arm_realtime_absolute_ns(&self, deadline_ns: u64) -> Result<(), LinuxTimerFdArmError> {
        let mut syscalls = syscall::LinuxTimerFdSyscalls;
        set_linux_timerfd_realtime_absolute(&mut syscalls, self.fd.as_raw_fd(), deadline_ns)
    }

    pub fn disarm(&self) -> Result<(), LinuxTimerFdArmError> {
        let mut syscalls = syscall::LinuxTimerFdSyscalls;
        syscall::set_linux_timerfd(
            &mut syscalls,
            self.fd.as_raw_fd(),
            0,
            LinuxTimerSpec::disarmed(),
        )
    }

    pub fn read_expirations(&self) -> Result<LinuxTimerFdRead, LinuxTimerFdReadError> {
        let mut syscalls = syscall::LinuxTimerFdSyscalls;
        read_linux_timerfd(&mut syscalls, self.fd.as_raw_fd())
    }
}

impl ShutdownDeadlineTimer for LinuxTimerFd {
    fn arm_absolute_ns(&mut self, deadline_ns: u64) -> Result<(), BoundaryError> {
        let mut syscalls = syscall::LinuxTimerFdSyscalls;
        set_linux_timerfd_absolute(&mut syscalls, self.fd.as_raw_fd(), deadline_ns)
            .map_err(|error| BoundaryError::Timer(format!("{error:?}")))
    }

    fn disarm(&mut self) -> Result<(), BoundaryError> {
        let mut syscalls = syscall::LinuxTimerFdSyscalls;
        syscall::set_linux_timerfd(
            &mut syscalls,
            self.fd.as_raw_fd(),
            0,
            LinuxTimerSpec::disarmed(),
        )
        .map_err(|error| BoundaryError::Timer(format!("{error:?}")))
    }
}

#[cfg(test)]
mod tests;
