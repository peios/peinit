use std::io;

use crate::shutdown::ShutdownSignal;

pub(super) const KERNEL_SIGSET_SIZE_BYTES: usize = 8;
pub(crate) const PID1_SIGNALFD_CREATE_FD: i32 = -1;
pub(crate) const PID1_SIGNALFD_FLAGS: i32 = libc::SFD_CLOEXEC | libc::SFD_NONBLOCK;
const LINUX_SIGNAL_MAX: i32 = 64;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinuxSignalMask(u64);

impl LinuxSignalMask {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn all_blockable() -> Self {
        let sigkill = 1_u64 << ((libc::SIGKILL as u32) - 1);
        let sigstop = 1_u64 << ((libc::SIGSTOP as u32) - 1);
        Self(!sigkill & !sigstop)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub fn contains(self, signal: i32) -> bool {
        if !(1..=LINUX_SIGNAL_MAX).contains(&signal) {
            return false;
        }
        self.0 & (1_u64 << ((signal as u32) - 1)) != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pid1SignalFdSetup {
    pub fd: i32,
    pub mask: LinuxSignalMask,
    pub flags: i32,
}

#[derive(Debug)]
pub enum Pid1SignalFdSetupError {
    Sigprocmask {
        how: i32,
        source: io::Error,
    },
    Signalfd {
        fd: i32,
        flags: i32,
        source: io::Error,
    },
    FdOutOfRange {
        returned_fd: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxSignalFdRead {
    Shutdown(ShutdownSignal),
    Other { signal: i32 },
    WouldBlock,
}

#[derive(Debug)]
pub enum LinuxSignalFdReadError {
    Read(io::Error),
    ShortRead { bytes: usize },
}
