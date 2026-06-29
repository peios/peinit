use std::io;

use super::model::{
    KERNEL_SIGSET_SIZE_BYTES, LinuxSignalFdRead, LinuxSignalFdReadError, LinuxSignalMask,
    PID1_SIGNALFD_CREATE_FD, PID1_SIGNALFD_FLAGS, Pid1SignalFdSetup, Pid1SignalFdSetupError,
};
use crate::boundary::linux_io::{is_would_block, short_read_error};
use crate::shutdown::ShutdownSignal;

pub fn setup_pid1_signalfd<S>(syscalls: &mut S) -> Result<Pid1SignalFdSetup, Pid1SignalFdSetupError>
where
    S: Pid1SignalFdSyscalls + ?Sized,
{
    let mask = LinuxSignalMask::all_blockable();
    let how = libc::SIG_BLOCK;
    syscalls
        .rt_sigprocmask(how, &mask)
        .map_err(|source| Pid1SignalFdSetupError::Sigprocmask { how, source })?;

    let fd = PID1_SIGNALFD_CREATE_FD;
    let flags = PID1_SIGNALFD_FLAGS;
    let returned_fd = syscalls
        .signalfd4(fd, &mask, flags)
        .map_err(|source| Pid1SignalFdSetupError::Signalfd { fd, flags, source })?;
    let fd = i32::try_from(returned_fd)
        .map_err(|_| Pid1SignalFdSetupError::FdOutOfRange { returned_fd })?;

    Ok(Pid1SignalFdSetup { fd, mask, flags })
}

pub(crate) fn read_pid1_signalfd<S>(
    syscalls: &mut S,
    fd: i32,
) -> Result<LinuxSignalFdRead, LinuxSignalFdReadError>
where
    S: Pid1SignalFdSyscalls + ?Sized,
{
    let info = match syscalls.read_signalfd(fd) {
        Ok(Some(info)) => info,
        Ok(None) => return Ok(LinuxSignalFdRead::WouldBlock),
        Err(source) => return Err(LinuxSignalFdReadError::Read(source)),
    };
    Ok(match info.ssi_signo as i32 {
        libc::SIGINT => LinuxSignalFdRead::Shutdown(ShutdownSignal::Sigint),
        libc::SIGTERM => LinuxSignalFdRead::Shutdown(ShutdownSignal::Sigterm),
        signal => LinuxSignalFdRead::Other { signal },
    })
}

pub trait Pid1SignalFdSyscalls {
    fn rt_sigprocmask(&mut self, how: i32, mask: &LinuxSignalMask) -> io::Result<()>;

    fn signalfd4(&mut self, fd: i32, mask: &LinuxSignalMask, flags: i32) -> io::Result<i64>;

    fn read_signalfd(&mut self, fd: i32) -> io::Result<Option<libc::signalfd_siginfo>>;

    fn close_fd(&mut self, fd: i32) -> io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LinuxSignalSyscalls;

impl Pid1SignalFdSyscalls for LinuxSignalSyscalls {
    fn rt_sigprocmask(&mut self, how: i32, mask: &LinuxSignalMask) -> io::Result<()> {
        let rc = unsafe {
            libc::syscall(
                libc::SYS_rt_sigprocmask,
                how,
                mask as *const LinuxSignalMask,
                std::ptr::null_mut::<LinuxSignalMask>(),
                KERNEL_SIGSET_SIZE_BYTES,
            )
        };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn signalfd4(&mut self, fd: i32, mask: &LinuxSignalMask, flags: i32) -> io::Result<i64> {
        let rc = unsafe {
            libc::syscall(
                libc::SYS_signalfd4,
                fd,
                mask as *const LinuxSignalMask,
                KERNEL_SIGSET_SIZE_BYTES,
                flags,
            )
        };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(rc as i64)
        }
    }

    fn read_signalfd(&mut self, fd: i32) -> io::Result<Option<libc::signalfd_siginfo>> {
        let mut info = unsafe { std::mem::zeroed::<libc::signalfd_siginfo>() };
        let expected = std::mem::size_of::<libc::signalfd_siginfo>();
        let rc = unsafe {
            libc::read(
                fd,
                (&mut info as *mut libc::signalfd_siginfo).cast::<libc::c_void>(),
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
            Ok(Some(info))
        } else {
            Err(short_read_error("signalfd", rc as usize, expected))
        }
    }

    fn close_fd(&mut self, fd: i32) -> io::Result<()> {
        let rc = unsafe { libc::close(fd) };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
