use std::io;

pub(crate) const EPOLL_CREATE_FLAGS: i32 = libc::EPOLL_CLOEXEC;
const EPOLL_READ_FLAGS: u32 = libc::EPOLLIN as u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxEpollEvent {
    pub events: u32,
    pub token: u64,
}

impl LinuxEpollEvent {
    pub const fn read(token: u64) -> Self {
        Self {
            events: EPOLL_READ_FLAGS,
            token,
        }
    }
}

#[derive(Debug)]
pub enum LinuxEpollCreateError {
    Create { flags: i32, source: io::Error },
    FdOutOfRange { returned_fd: i64 },
}

#[derive(Debug)]
pub enum LinuxEpollRegisterError {
    Control {
        epoll_fd: i32,
        op: i32,
        fd: i32,
        event: LinuxEpollEvent,
        source: io::Error,
    },
}

#[derive(Debug)]
pub enum LinuxEpollUnregisterError {
    Control {
        epoll_fd: i32,
        op: i32,
        fd: i32,
        source: io::Error,
    },
}

#[derive(Debug)]
pub enum LinuxEpollWaitError {
    InvalidMaxEvents,
    Wait {
        epoll_fd: i32,
        timeout_ms: i32,
        source: io::Error,
    },
}
