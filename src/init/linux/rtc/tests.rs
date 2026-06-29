use std::collections::BTreeMap;
use std::path::Path;

use super::{RtcClockSyscalls, RtcTime, rtc_time_to_unix_seconds, set_clock_from_hardware_rtc};

#[derive(Debug, Clone, PartialEq, Eq)]
enum RtcCall {
    Open(String),
    Read(i32),
    Close(i32),
    SetRealtime { seconds: i64, nanoseconds: i64 },
}

#[derive(Debug)]
struct FakeRtcSyscalls {
    calls: Vec<RtcCall>,
    open_failures: BTreeMap<String, i32>,
    next_fd: i32,
    rtc_time: RtcTime,
    read_failure: Option<i32>,
    close_failure: Option<i32>,
    set_failure: Option<i32>,
}

impl FakeRtcSyscalls {
    fn new(rtc_time: RtcTime) -> Self {
        Self {
            calls: Vec::new(),
            open_failures: BTreeMap::new(),
            next_fd: 10,
            rtc_time,
            read_failure: None,
            close_failure: None,
            set_failure: None,
        }
    }

    fn fail_open(mut self, path: &str, errno: i32) -> Self {
        self.open_failures.insert(path.to_string(), errno);
        self
    }

    fn fail_close(mut self, errno: i32) -> Self {
        self.close_failure = Some(errno);
        self
    }
}

impl RtcClockSyscalls for FakeRtcSyscalls {
    fn open_rtc_device(&mut self, path: &Path) -> std::io::Result<i32> {
        self.calls.push(RtcCall::Open(path.display().to_string()));
        if let Some(errno) = self.open_failures.get(&path.display().to_string()) {
            Err(std::io::Error::from_raw_os_error(*errno))
        } else {
            let fd = self.next_fd;
            self.next_fd += 1;
            Ok(fd)
        }
    }

    fn read_rtc_time(&mut self, fd: i32) -> std::io::Result<RtcTime> {
        self.calls.push(RtcCall::Read(fd));
        if let Some(errno) = self.read_failure {
            Err(std::io::Error::from_raw_os_error(errno))
        } else {
            Ok(self.rtc_time)
        }
    }

    fn close_fd(&mut self, fd: i32) -> std::io::Result<()> {
        self.calls.push(RtcCall::Close(fd));
        if let Some(errno) = self.close_failure {
            Err(std::io::Error::from_raw_os_error(errno))
        } else {
            Ok(())
        }
    }

    fn set_realtime(&mut self, seconds: i64, nanoseconds: i64) -> std::io::Result<()> {
        self.calls.push(RtcCall::SetRealtime {
            seconds,
            nanoseconds,
        });
        if let Some(errno) = self.set_failure {
            Err(std::io::Error::from_raw_os_error(errno))
        } else {
            Ok(())
        }
    }
}

fn leap_day_time() -> RtcTime {
    RtcTime {
        sec: 3,
        min: 2,
        hour: 1,
        mday: 29,
        mon: 1,
        year: 124,
    }
}

#[test]
fn converts_valid_rtc_time_to_utc_unix_seconds() {
    assert_eq!(
        rtc_time_to_unix_seconds(leap_day_time()).expect("timestamp"),
        1_709_168_523,
    );
}

#[test]
fn falls_back_to_rtc0_only_when_primary_is_absent() {
    let mut syscalls = FakeRtcSyscalls::new(leap_day_time()).fail_open("/dev/rtc", libc::ENOENT);

    set_clock_from_hardware_rtc(&mut syscalls).expect("rtc setup");

    assert_eq!(
        syscalls.calls,
        vec![
            RtcCall::Open("/dev/rtc".to_string()),
            RtcCall::Open("/dev/rtc0".to_string()),
            RtcCall::Read(10),
            RtcCall::Close(10),
            RtcCall::SetRealtime {
                seconds: 1_709_168_523,
                nanoseconds: 0,
            },
        ]
    );
}

#[test]
fn does_not_try_fallback_when_primary_open_fails_for_permission() {
    let mut syscalls = FakeRtcSyscalls::new(leap_day_time()).fail_open("/dev/rtc", libc::EACCES);

    let error = set_clock_from_hardware_rtc(&mut syscalls).expect_err("open failure");

    assert!(format!("{error:?}").contains("open /dev/rtc failed"));
    assert_eq!(syscalls.calls, vec![RtcCall::Open("/dev/rtc".to_string())]);
}

#[test]
fn rejects_invalid_or_pre_epoch_time() {
    let mut invalid = FakeRtcSyscalls::new(RtcTime {
        sec: 0,
        min: 0,
        hour: 0,
        mday: 29,
        mon: 1,
        year: 123,
    });

    let error = set_clock_from_hardware_rtc(&mut invalid).expect_err("invalid date");

    assert!(format!("{error:?}").contains("day 29 out of range"));
}

#[test]
fn close_failure_after_successful_read_is_recovery_failure() {
    let mut syscalls = FakeRtcSyscalls::new(leap_day_time()).fail_close(libc::EIO);

    let error = set_clock_from_hardware_rtc(&mut syscalls).expect_err("close failure");

    assert!(format!("{error:?}").contains("close RTC fd 10 after successful read failed"));
    assert!(!syscalls.calls.iter().any(|call| {
        matches!(
            call,
            RtcCall::SetRealtime {
                seconds: _,
                nanoseconds: _
            }
        )
    }));
}
