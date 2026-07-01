use std::io;

#[cfg(feature = "peios-boundary")]
use crate::boundary::linux_io::is_would_block;

use super::model::{EV_KEY, KEY_POWER, LinuxPowerButtonRead, LinuxPowerButtonReadError};

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LinuxInputEvent {
    pub time: libc::timeval,
    pub event_type: u16,
    pub code: u16,
    pub value: i32,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum LinuxPowerButtonInputRead {
    Event(LinuxInputEvent),
    WouldBlock,
    Short { bytes: usize },
}

pub fn read_linux_power_button_event<S>(
    syscalls: &mut S,
    fd: i32,
) -> Result<LinuxPowerButtonRead, LinuxPowerButtonReadError>
where
    S: LinuxPowerButtonInputSyscalls + ?Sized,
{
    match syscalls.read_input_event(fd) {
        Ok(LinuxPowerButtonInputRead::Event(event)) => Ok(power_button_read_from_event(event)),
        Ok(LinuxPowerButtonInputRead::WouldBlock) => Ok(LinuxPowerButtonRead::WouldBlock),
        Ok(LinuxPowerButtonInputRead::Short { bytes }) => {
            Err(LinuxPowerButtonReadError::ShortRead {
                fd,
                bytes,
                expected: std::mem::size_of::<LinuxInputEvent>(),
            })
        }
        Err(source) => Err(LinuxPowerButtonReadError::Read {
            fd,
            message: source.to_string(),
        }),
    }
}

pub(super) trait LinuxPowerButtonInputSyscalls {
    fn read_input_event(&mut self, fd: i32) -> io::Result<LinuxPowerButtonInputRead>;
}

#[cfg(feature = "peios-boundary")]
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LinuxPowerButtonSyscalls;

#[cfg(feature = "peios-boundary")]
impl LinuxPowerButtonInputSyscalls for LinuxPowerButtonSyscalls {
    fn read_input_event(&mut self, fd: i32) -> io::Result<LinuxPowerButtonInputRead> {
        let mut event = LinuxInputEvent::default();
        let expected = std::mem::size_of::<LinuxInputEvent>();
        let read = unsafe { libc::read(fd, (&mut event as *mut LinuxInputEvent).cast(), expected) };
        if read < 0 {
            let error = io::Error::last_os_error();
            if is_would_block(&error) {
                Ok(LinuxPowerButtonInputRead::WouldBlock)
            } else {
                Err(error)
            }
        } else if read as usize == expected {
            Ok(LinuxPowerButtonInputRead::Event(event))
        } else {
            Ok(LinuxPowerButtonInputRead::Short {
                bytes: read as usize,
            })
        }
    }
}

fn power_button_read_from_event(event: LinuxInputEvent) -> LinuxPowerButtonRead {
    if event.event_type == EV_KEY && event.code == KEY_POWER && event.value == 1 {
        LinuxPowerButtonRead::Pressed
    } else {
        LinuxPowerButtonRead::Ignored
    }
}
