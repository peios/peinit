use std::io;
use std::os::fd::{FromRawFd, OwnedFd};

use crate::boundary::linux_epoll::{
    LinuxEpoll, LinuxEpollRegisterError, LinuxEpollSyscallApi, register_linux_epoll_read,
};
use crate::boundary::linux_signal::{
    LinuxPid1SignalFd, LinuxSignalSyscalls, Pid1SignalFdSetup, Pid1SignalFdSetupError,
    Pid1SignalFdSyscalls, setup_pid1_signalfd,
};

impl LinuxPid1SignalFd {
    pub fn setup_registered(
        epoll: &LinuxEpoll,
        token: u64,
    ) -> Result<Self, Pid1SignalFdRegisteredSetupError> {
        let mut syscalls = LinuxSignalSyscalls;
        let setup = setup_pid1_signalfd_registered(&mut syscalls, epoll.as_raw_fd(), token)?;
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(setup.signal.fd) },
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pid1SignalFdRegisteredSetup {
    pub signal: Pid1SignalFdSetup,
    pub epoll_fd: i32,
    pub token: u64,
}

#[derive(Debug)]
pub enum Pid1SignalFdRegisteredSetupError {
    Signal(Pid1SignalFdSetupError),
    Register {
        source: LinuxEpollRegisterError,
        close_error: Option<io::Error>,
    },
}

pub fn setup_pid1_signalfd_registered<S>(
    syscalls: &mut S,
    epoll_fd: i32,
    token: u64,
) -> Result<Pid1SignalFdRegisteredSetup, Pid1SignalFdRegisteredSetupError>
where
    S: Pid1SignalFdSyscalls + LinuxEpollSyscallApi + ?Sized,
{
    let signal = setup_pid1_signalfd(syscalls).map_err(Pid1SignalFdRegisteredSetupError::Signal)?;
    if let Err(source) = register_linux_epoll_read(syscalls, epoll_fd, signal.fd, token) {
        let close_error = syscalls.close_fd(signal.fd).err();
        return Err(Pid1SignalFdRegisteredSetupError::Register {
            source,
            close_error,
        });
    }

    Ok(Pid1SignalFdRegisteredSetup {
        signal,
        epoll_fd,
        token,
    })
}

impl LinuxEpollSyscallApi for LinuxSignalSyscalls {
    fn monotonic_ns(&mut self) -> io::Result<u64> {
        crate::boundary::linux_epoll::clock_monotonic_ns()
    }

    fn epoll_create1(&mut self, flags: i32) -> io::Result<i64> {
        let fd = unsafe { libc::epoll_create1(flags) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(i64::from(fd))
        }
    }

    fn epoll_ctl(
        &mut self,
        epoll_fd: i32,
        op: i32,
        fd: i32,
        event: Option<crate::boundary::linux_epoll::LinuxEpollEvent>,
    ) -> io::Result<()> {
        let mut raw_event = event.map(|event| libc::epoll_event {
            events: event.events,
            u64: event.token,
        });
        let event_ptr = raw_event.as_mut().map_or(std::ptr::null_mut(), |event| {
            event as *mut libc::epoll_event
        });
        let rc = unsafe { libc::epoll_ctl(epoll_fd, op, fd, event_ptr) };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn epoll_wait(
        &mut self,
        epoll_fd: i32,
        max_events: usize,
        timeout_ms: i32,
    ) -> io::Result<Vec<crate::boundary::linux_epoll::LinuxEpollEvent>> {
        let mut events = vec![libc::epoll_event { events: 0, u64: 0 }; max_events];
        let rc = unsafe {
            libc::epoll_wait(
                epoll_fd,
                events.as_mut_ptr(),
                events.len() as i32,
                timeout_ms,
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        events.truncate(rc as usize);
        Ok(events
            .into_iter()
            .map(|event| crate::boundary::linux_epoll::LinuxEpollEvent {
                events: event.events,
                token: event.u64,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests;
