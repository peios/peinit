use std::io;
use std::os::fd::IntoRawFd;
use std::path::Path;

use peios::file::{FileAccess, OpenOptions};

use super::RtcTime;

pub(in crate::init::linux) trait RtcClockSyscalls {
    fn open_rtc_device(&mut self, path: &Path) -> io::Result<i32>;
    fn read_rtc_time(&mut self, fd: i32) -> io::Result<RtcTime>;
    fn close_fd(&mut self, fd: i32) -> io::Result<()>;
    fn set_realtime(&mut self, seconds: i64, nanoseconds: i64) -> io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::init::linux) struct LinuxRtcClockSyscalls;

impl RtcClockSyscalls for LinuxRtcClockSyscalls {
    fn open_rtc_device(&mut self, path: &Path) -> io::Result<i32> {
        let file = OpenOptions::new()
            .desired_access(FileAccess::READ_DATA)
            .open(None, path)
            .map_err(io::Error::from)?;
        Ok(file.into_raw_fd())
    }

    fn read_rtc_time(&mut self, fd: i32) -> io::Result<RtcTime> {
        let mut raw = RawRtcTime::default();
        let rc = unsafe { libc::ioctl(fd, rtc_rd_time_request(), &mut raw) };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(RtcTime {
                sec: raw.tm_sec,
                min: raw.tm_min,
                hour: raw.tm_hour,
                mday: raw.tm_mday,
                mon: raw.tm_mon,
                year: raw.tm_year,
            })
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

    fn set_realtime(&mut self, seconds: i64, nanoseconds: i64) -> io::Result<()> {
        let timespec = libc::timespec {
            tv_sec: seconds as libc::time_t,
            tv_nsec: nanoseconds as libc::c_long,
        };
        let rc = unsafe { libc::clock_settime(libc::CLOCK_REALTIME, &timespec) };
        if rc < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct RawRtcTime {
    tm_sec: libc::c_int,
    tm_min: libc::c_int,
    tm_hour: libc::c_int,
    tm_mday: libc::c_int,
    tm_mon: libc::c_int,
    tm_year: libc::c_int,
    tm_wday: libc::c_int,
    tm_yday: libc::c_int,
    tm_isdst: libc::c_int,
}

const IOC_NRBITS: u32 = 8;
const IOC_TYPEBITS: u32 = 8;
const IOC_SIZEBITS: u32 = 14;
const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;
const IOC_READ: u32 = 2;

const fn ioc(dir: u32, ty: u8, nr: u8, size: usize) -> libc::c_ulong {
    ((dir as libc::c_ulong) << IOC_DIRSHIFT)
        | ((ty as libc::c_ulong) << IOC_TYPESHIFT)
        | ((nr as libc::c_ulong) << IOC_NRSHIFT)
        | ((size as libc::c_ulong) << IOC_SIZESHIFT)
}

const fn rtc_rd_time_request() -> libc::c_ulong {
    ioc(IOC_READ, b'p', 0x09, std::mem::size_of::<RawRtcTime>())
}
