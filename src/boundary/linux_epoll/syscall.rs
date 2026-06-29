use std::io;

use super::model::{
    EPOLL_CREATE_FLAGS, LinuxEpollCreateError, LinuxEpollEvent, LinuxEpollRegisterError,
    LinuxEpollUnregisterError, LinuxEpollWaitError,
};

pub fn create_linux_epoll<S>(syscalls: &mut S) -> Result<i32, LinuxEpollCreateError>
where
    S: LinuxEpollSyscallApi + ?Sized,
{
    let returned_fd = syscalls
        .epoll_create1(EPOLL_CREATE_FLAGS)
        .map_err(|source| LinuxEpollCreateError::Create {
            flags: EPOLL_CREATE_FLAGS,
            source,
        })?;
    i32::try_from(returned_fd).map_err(|_| LinuxEpollCreateError::FdOutOfRange { returned_fd })
}

pub fn register_linux_epoll_read<S>(
    syscalls: &mut S,
    epoll_fd: i32,
    fd: i32,
    token: u64,
) -> Result<(), LinuxEpollRegisterError>
where
    S: LinuxEpollSyscallApi + ?Sized,
{
    let event = LinuxEpollEvent::read(token);
    syscalls
        .epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, fd, Some(event))
        .map_err(|source| LinuxEpollRegisterError::Control {
            epoll_fd,
            op: libc::EPOLL_CTL_ADD,
            fd,
            event,
            source,
        })
}

pub fn unregister_linux_epoll<S>(
    syscalls: &mut S,
    epoll_fd: i32,
    fd: i32,
) -> Result<(), LinuxEpollUnregisterError>
where
    S: LinuxEpollSyscallApi + ?Sized,
{
    syscalls
        .epoll_ctl(epoll_fd, libc::EPOLL_CTL_DEL, fd, None)
        .map_err(|source| LinuxEpollUnregisterError::Control {
            epoll_fd,
            op: libc::EPOLL_CTL_DEL,
            fd,
            source,
        })
}

pub fn wait_linux_epoll<S>(
    syscalls: &mut S,
    epoll_fd: i32,
    max_events: usize,
    timeout_ms: i32,
) -> Result<Vec<LinuxEpollEvent>, LinuxEpollWaitError>
where
    S: LinuxEpollSyscallApi + ?Sized,
{
    if max_events == 0 {
        return Err(LinuxEpollWaitError::InvalidMaxEvents);
    }
    syscalls
        .epoll_wait(epoll_fd, max_events, timeout_ms)
        .map_err(|source| LinuxEpollWaitError::Wait {
            epoll_fd,
            timeout_ms,
            source,
        })
}

pub trait LinuxEpollSyscallApi {
    fn epoll_create1(&mut self, flags: i32) -> io::Result<i64>;

    fn epoll_ctl(
        &mut self,
        epoll_fd: i32,
        op: i32,
        fd: i32,
        event: Option<LinuxEpollEvent>,
    ) -> io::Result<()>;

    fn epoll_wait(
        &mut self,
        epoll_fd: i32,
        max_events: usize,
        timeout_ms: i32,
    ) -> io::Result<Vec<LinuxEpollEvent>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LinuxEpollSyscalls;

impl LinuxEpollSyscallApi for LinuxEpollSyscalls {
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
        event: Option<LinuxEpollEvent>,
    ) -> io::Result<()> {
        let mut raw_event = event.map(raw_epoll_event);
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
    ) -> io::Result<Vec<LinuxEpollEvent>> {
        let mut events = vec![raw_epoll_event(LinuxEpollEvent::read(0)); max_events];
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
        Ok(events.into_iter().map(linux_epoll_event).collect())
    }
}

fn raw_epoll_event(event: LinuxEpollEvent) -> libc::epoll_event {
    libc::epoll_event {
        events: event.events,
        u64: event.token,
    }
}

fn linux_epoll_event(event: libc::epoll_event) -> LinuxEpollEvent {
    LinuxEpollEvent {
        events: event.events,
        token: event.u64,
    }
}
