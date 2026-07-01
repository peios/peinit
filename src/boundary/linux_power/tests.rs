use std::collections::VecDeque;
use std::io;

use super::model::{EV_KEY, KEY_POWER, LinuxPowerButtonRead, LinuxPowerButtonReadError};
use super::syscall::{
    LinuxInputEvent, LinuxPowerButtonInputRead, LinuxPowerButtonInputSyscalls,
    read_linux_power_button_event,
};

#[test]
fn power_button_press_is_reported() {
    let mut syscalls = FakePowerButtonSyscalls::new([Ok(LinuxPowerButtonInputRead::Event(
        input_event(EV_KEY, KEY_POWER, 1),
    ))]);

    assert_eq!(
        read_linux_power_button_event(&mut syscalls, 17).expect("read"),
        LinuxPowerButtonRead::Pressed,
    );
    assert_eq!(syscalls.read_fds, vec![17]);
}

#[test]
fn power_button_release_repeat_and_other_keys_are_ignored() {
    let mut syscalls = FakePowerButtonSyscalls::new([
        Ok(LinuxPowerButtonInputRead::Event(input_event(
            EV_KEY, KEY_POWER, 0,
        ))),
        Ok(LinuxPowerButtonInputRead::Event(input_event(
            EV_KEY, KEY_POWER, 2,
        ))),
        Ok(LinuxPowerButtonInputRead::Event(input_event(EV_KEY, 1, 1))),
        Ok(LinuxPowerButtonInputRead::Event(input_event(
            0, KEY_POWER, 1,
        ))),
    ]);

    for _ in 0..4 {
        assert_eq!(
            read_linux_power_button_event(&mut syscalls, 17).expect("read"),
            LinuxPowerButtonRead::Ignored,
        );
    }
}

#[test]
fn power_button_read_preserves_would_block() {
    let mut syscalls = FakePowerButtonSyscalls::new([Ok(LinuxPowerButtonInputRead::WouldBlock)]);

    assert_eq!(
        read_linux_power_button_event(&mut syscalls, 17).expect("read"),
        LinuxPowerButtonRead::WouldBlock,
    );
}

#[test]
fn power_button_read_reports_short_reads_and_errors() {
    let mut short =
        FakePowerButtonSyscalls::new([Ok(LinuxPowerButtonInputRead::Short { bytes: 3 })]);

    assert_eq!(
        read_linux_power_button_event(&mut short, 17).expect_err("short read"),
        LinuxPowerButtonReadError::ShortRead {
            fd: 17,
            bytes: 3,
            expected: std::mem::size_of::<LinuxInputEvent>(),
        },
    );

    let mut error =
        FakePowerButtonSyscalls::new([Err(io::Error::from(io::ErrorKind::PermissionDenied))]);

    assert!(matches!(
        read_linux_power_button_event(&mut error, 18).expect_err("read error"),
        LinuxPowerButtonReadError::Read { fd: 18, .. },
    ));
}

#[derive(Debug)]
struct FakePowerButtonSyscalls {
    reads: VecDeque<io::Result<LinuxPowerButtonInputRead>>,
    read_fds: Vec<i32>,
}

impl FakePowerButtonSyscalls {
    fn new(reads: impl IntoIterator<Item = io::Result<LinuxPowerButtonInputRead>>) -> Self {
        Self {
            reads: reads.into_iter().collect(),
            read_fds: Vec::new(),
        }
    }
}

impl LinuxPowerButtonInputSyscalls for FakePowerButtonSyscalls {
    fn read_input_event(&mut self, fd: i32) -> io::Result<LinuxPowerButtonInputRead> {
        self.read_fds.push(fd);
        self.reads
            .pop_front()
            .unwrap_or(Ok(LinuxPowerButtonInputRead::WouldBlock))
    }
}

fn input_event(event_type: u16, code: u16, value: i32) -> LinuxInputEvent {
    LinuxInputEvent {
        time: libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        event_type,
        code,
        value,
    }
}
