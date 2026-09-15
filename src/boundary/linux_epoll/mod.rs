use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

mod model;
mod syscall;

pub use model::{
    LinuxEpollCreateError, LinuxEpollEvent, LinuxEpollRegisterError, LinuxEpollUnregisterError,
    LinuxEpollWaitError,
};
pub(in crate::boundary) use syscall::clock_monotonic_ns;
pub use syscall::{
    LinuxEpollSyscallApi, create_linux_epoll, register_linux_epoll_read, unregister_linux_epoll,
    wait_linux_epoll,
};

#[cfg(test)]
pub(super) use model::EPOLL_CREATE_FLAGS;

#[derive(Debug)]
pub struct LinuxEpoll {
    fd: OwnedFd,
}

impl LinuxEpoll {
    pub fn create() -> Result<Self, LinuxEpollCreateError> {
        let mut syscalls = syscall::LinuxEpollSyscalls;
        let fd = create_linux_epoll(&mut syscalls)?;
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }

    pub fn as_raw_fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    pub fn register_read(&self, fd: i32, token: u64) -> Result<(), LinuxEpollRegisterError> {
        let mut syscalls = syscall::LinuxEpollSyscalls;
        register_linux_epoll_read(&mut syscalls, self.fd.as_raw_fd(), fd, token)
    }

    pub fn unregister(&self, fd: i32) -> Result<(), LinuxEpollUnregisterError> {
        let mut syscalls = syscall::LinuxEpollSyscalls;
        unregister_linux_epoll(&mut syscalls, self.fd.as_raw_fd(), fd)
    }

    pub fn wait(
        &self,
        max_events: usize,
        timeout_ms: i32,
    ) -> Result<Vec<LinuxEpollEvent>, LinuxEpollWaitError> {
        let mut syscalls = syscall::LinuxEpollSyscalls;
        wait_linux_epoll(&mut syscalls, self.fd.as_raw_fd(), max_events, timeout_ms)
    }
}

#[cfg(test)]
mod tests;
