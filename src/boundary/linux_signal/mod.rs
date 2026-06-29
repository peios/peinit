use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

mod model;
mod registered;
mod syscall;

pub use model::{
    LinuxSignalFdRead, LinuxSignalFdReadError, LinuxSignalMask, Pid1SignalFdSetup,
    Pid1SignalFdSetupError,
};
pub use registered::{
    Pid1SignalFdRegisteredSetup, Pid1SignalFdRegisteredSetupError, setup_pid1_signalfd_registered,
};
pub use syscall::{Pid1SignalFdSyscalls, setup_pid1_signalfd};

#[cfg(test)]
pub(super) use model::{PID1_SIGNALFD_CREATE_FD, PID1_SIGNALFD_FLAGS};
pub(super) use syscall::LinuxSignalSyscalls;
#[cfg(test)]
pub(super) use syscall::read_pid1_signalfd;

#[derive(Debug)]
pub struct LinuxPid1SignalFd {
    fd: OwnedFd,
}

impl LinuxPid1SignalFd {
    pub fn setup() -> Result<Self, Pid1SignalFdSetupError> {
        let mut syscalls = LinuxSignalSyscalls;
        let setup = setup_pid1_signalfd(&mut syscalls)?;
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(setup.fd) },
        })
    }

    pub fn as_raw_fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    pub fn read_signal(&self) -> Result<LinuxSignalFdRead, LinuxSignalFdReadError> {
        let mut syscalls = LinuxSignalSyscalls;
        syscall::read_pid1_signalfd(&mut syscalls, self.fd.as_raw_fd())
    }
}

#[cfg(test)]
mod tests;
