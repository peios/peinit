use std::io;

use super::model::{
    LinuxTimerFdArmError, LinuxTimerFdCreateError, LinuxTimerFdRead, LinuxTimerFdReadError,
    LinuxTimerSpec, TIMERFD_ABSOLUTE_FLAGS, TIMERFD_CREATE_CLOCK, TIMERFD_CREATE_FLAGS,
    TIMERFD_REALTIME_ABSOLUTE_FLAGS, TIMERFD_REALTIME_CREATE_CLOCK,
    linux_timer_spec_from_deadline_ns,
};
use crate::boundary::linux_io::{is_would_block, short_read_error};

pub fn create_linux_monotonic_timerfd<S>(syscalls: &mut S) -> Result<i32, LinuxTimerFdCreateError>
where
    S: LinuxTimerFdSyscallApi + ?Sized,
{
    create_linux_timerfd(syscalls, TIMERFD_CREATE_CLOCK)
}

pub fn create_linux_realtime_timerfd<S>(syscalls: &mut S) -> Result<i32, LinuxTimerFdCreateError>
where
    S: LinuxTimerFdSyscallApi + ?Sized,
{
    create_linux_timerfd(syscalls, TIMERFD_REALTIME_CREATE_CLOCK)
}

fn create_linux_timerfd<S>(syscalls: &mut S, clock_id: i32) -> Result<i32, LinuxTimerFdCreateError>
where
    S: LinuxTimerFdSyscallApi + ?Sized,
{
    let returned_fd = syscalls
        .timerfd_create(clock_id, TIMERFD_CREATE_FLAGS)
        .map_err(|source| LinuxTimerFdCreateError::Create {
            clock_id,
            flags: TIMERFD_CREATE_FLAGS,
            source,
        })?;
    i32::try_from(returned_fd).map_err(|_| LinuxTimerFdCreateError::FdOutOfRange { returned_fd })
}

pub fn set_linux_timerfd_absolute<S>(
    syscalls: &mut S,
    fd: i32,
    deadline_ns: u64,
) -> Result<(), LinuxTimerFdArmError>
where
    S: LinuxTimerFdSyscallApi + ?Sized,
{
    let spec = linux_timer_spec_from_deadline_ns(deadline_ns);
    set_linux_timerfd(syscalls, fd, TIMERFD_ABSOLUTE_FLAGS, spec)
}

pub fn set_linux_timerfd_realtime_absolute<S>(
    syscalls: &mut S,
    fd: i32,
    deadline_ns: u64,
) -> Result<(), LinuxTimerFdArmError>
where
    S: LinuxTimerFdSyscallApi + ?Sized,
{
    let spec = linux_timer_spec_from_deadline_ns(deadline_ns);
    set_linux_timerfd(syscalls, fd, TIMERFD_REALTIME_ABSOLUTE_FLAGS, spec)
}

pub(super) fn set_linux_timerfd<S>(
    syscalls: &mut S,
    fd: i32,
    flags: i32,
    spec: LinuxTimerSpec,
) -> Result<(), LinuxTimerFdArmError>
where
    S: LinuxTimerFdSyscallApi + ?Sized,
{
    syscalls
        .timerfd_settime(fd, flags, spec)
        .map_err(|source| LinuxTimerFdArmError::SetTime {
            fd,
            flags,
            spec,
            source,
        })
}

pub fn read_linux_timerfd<S>(
    syscalls: &mut S,
    fd: i32,
) -> Result<LinuxTimerFdRead, LinuxTimerFdReadError>
where
    S: LinuxTimerFdSyscallApi + ?Sized,
{
    match syscalls.read_timerfd(fd) {
        Ok(Some(expirations)) => Ok(LinuxTimerFdRead::Expired { expirations }),
        Ok(None) => Ok(LinuxTimerFdRead::WouldBlock),
        Err(source) if source.raw_os_error() == Some(libc::ECANCELED) => {
            Ok(LinuxTimerFdRead::Canceled)
        }
        Err(source) => Err(LinuxTimerFdReadError::Read(source)),
    }
}

pub trait LinuxTimerFdSyscallApi {
    fn timerfd_create(&mut self, clock_id: i32, flags: i32) -> io::Result<i64>;

    fn timerfd_settime(&mut self, fd: i32, flags: i32, spec: LinuxTimerSpec) -> io::Result<()>;

    fn read_timerfd(&mut self, fd: i32) -> io::Result<Option<u64>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LinuxTimerFdSyscalls;

impl LinuxTimerFdSyscallApi for LinuxTimerFdSyscalls {
    fn timerfd_create(&mut self, clock_id: i32, flags: i32) -> io::Result<i64> {
        let fd = unsafe { libc::timerfd_create(clock_id, flags) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(i64::from(fd))
        }
    }

    fn timerfd_settime(&mut self, fd: i32, flags: i32, spec: LinuxTimerSpec) -> io::Result<()> {
        let raw = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: spec.interval_sec,
                tv_nsec: spec.interval_nsec,
            },
            it_value: libc::timespec {
                tv_sec: spec.initial_sec,
                tv_nsec: spec.initial_nsec,
            },
        };
        let rc = unsafe { libc::timerfd_settime(fd, flags, &raw, std::ptr::null_mut()) };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn read_timerfd(&mut self, fd: i32) -> io::Result<Option<u64>> {
        let mut expirations = 0_u64;
        let expected = std::mem::size_of::<u64>();
        let rc = unsafe {
            libc::read(
                fd,
                (&mut expirations as *mut u64).cast::<libc::c_void>(),
                expected,
            )
        };
        if rc < 0 {
            let error = io::Error::last_os_error();
            if is_would_block(&error) {
                Ok(None)
            } else {
                Err(error)
            }
        } else if rc as usize == expected {
            Ok(Some(expirations))
        } else {
            Err(short_read_error("timerfd", rc as usize, expected))
        }
    }
}
