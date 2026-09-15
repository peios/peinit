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
    // An interrupted wait is retried here, and never reaches the runtime loop.
    //
    // `epoll_wait` with a finite timeout fails EINTR after a ptrace stop even
    // when no signal handler ran (PEI-1085), and the wait is the one place
    // PID 1 spends nearly all of its time — so a debugger attaching to peinit
    // used to take the machine to recovery. The retry keeps the original
    // deadline rather than restarting the timeout: the clock kept running
    // while peinit was stopped, and a timer the loop is waiting on is due
    // when it is due.
    let deadline_ns = if timeout_ms > 0 {
        let started_at_ns = monotonic_ns(syscalls)?;
        Some(started_at_ns.saturating_add(u64::from(timeout_ms.unsigned_abs()) * NANOS_PER_MILLI))
    } else {
        None
    };
    let mut remaining_ms = timeout_ms;
    loop {
        match syscalls.epoll_wait(epoll_fd, max_events, remaining_ms) {
            Ok(events) => return Ok(events),
            Err(source) if source.kind() == io::ErrorKind::Interrupted => {
                if let Some(deadline_ns) = deadline_ns {
                    remaining_ms = remaining_timeout_ms(deadline_ns, monotonic_ns(syscalls)?);
                }
            }
            Err(source) => {
                return Err(LinuxEpollWaitError::Wait {
                    epoll_fd,
                    timeout_ms: remaining_ms,
                    source,
                });
            }
        }
    }
}

const NANOS_PER_MILLI: u64 = 1_000_000;

fn monotonic_ns<S>(syscalls: &mut S) -> Result<u64, LinuxEpollWaitError>
where
    S: LinuxEpollSyscallApi + ?Sized,
{
    syscalls
        .monotonic_ns()
        .map_err(|source| LinuxEpollWaitError::Clock { source })
}

/// Milliseconds left until `deadline_ns`, rounded up so a retry never
/// returns before the deadline; zero once it has passed.
fn remaining_timeout_ms(deadline_ns: u64, now_ns: u64) -> i32 {
    let remaining_ns = deadline_ns.saturating_sub(now_ns);
    let remaining_ms = remaining_ns.div_ceil(NANOS_PER_MILLI);
    i32::try_from(remaining_ms).unwrap_or(i32::MAX)
}

pub trait LinuxEpollSyscallApi {
    /// `CLOCK_MONOTONIC`, in nanoseconds: what the interrupted-wait retry
    /// measures its remaining timeout against.
    fn monotonic_ns(&mut self) -> io::Result<u64>;

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

/// `CLOCK_MONOTONIC` in nanoseconds, for the real syscall tables.
pub(in crate::boundary) fn clock_monotonic_ns() -> io::Result<u64> {
    let mut spec = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut spec) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    let sec = u64::try_from(spec.tv_sec)
        .map_err(|_| io::Error::other("CLOCK_MONOTONIC went negative"))?;
    let nsec = u64::try_from(spec.tv_nsec)
        .map_err(|_| io::Error::other("CLOCK_MONOTONIC nanoseconds went negative"))?;
    sec.checked_mul(1_000_000_000)
        .and_then(|ns| ns.checked_add(nsec))
        .ok_or_else(|| io::Error::other("CLOCK_MONOTONIC overflowed"))
}

impl LinuxEpollSyscallApi for LinuxEpollSyscalls {
    fn monotonic_ns(&mut self) -> io::Result<u64> {
        clock_monotonic_ns()
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
