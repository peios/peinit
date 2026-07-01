use std::fs;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;

use peios::file::{FileAccess, OpenOptions};

use super::model::{LinuxPowerButtonRead, LinuxPowerButtonReadError};
use super::set_cloexec_nonblocking;
use super::syscall::{LinuxPowerButtonSyscalls, read_linux_power_button_event};

const DEFAULT_INPUT_DIR: &str = "/dev/input";

#[derive(Debug, Default)]
pub struct LinuxPowerButtonDevices {
    devices: Vec<LinuxPowerButtonDevice>,
}

impl LinuxPowerButtonDevices {
    pub fn open_default() -> Self {
        Self::open_from_dir(Path::new(DEFAULT_INPUT_DIR))
    }

    pub fn fds(&self) -> impl Iterator<Item = i32> + '_ {
        self.devices.iter().map(LinuxPowerButtonDevice::fd)
    }

    pub fn retain_fds(&mut self, mut keep: impl FnMut(i32) -> bool) {
        self.devices.retain(|device| keep(device.fd()));
    }

    pub fn read_power_button(
        &mut self,
        fd: i32,
    ) -> Result<LinuxPowerButtonRead, LinuxPowerButtonReadError> {
        if !self.devices.iter().any(|device| device.fd() == fd) {
            return Err(LinuxPowerButtonReadError::StaleFd { fd });
        }
        let mut syscalls = LinuxPowerButtonSyscalls;
        read_linux_power_button_event(&mut syscalls, fd)
    }

    fn open_from_dir(input_dir: &Path) -> Self {
        let Ok(entries) = fs::read_dir(input_dir) else {
            return Self::default();
        };

        let mut devices = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_event_device_path(&path) {
                continue;
            }
            if let Some(fd) = open_event_device(&path) {
                devices.push(LinuxPowerButtonDevice { fd });
            }
        }
        Self { devices }
    }
}

#[derive(Debug)]
struct LinuxPowerButtonDevice {
    fd: OwnedFd,
}

impl LinuxPowerButtonDevice {
    fn fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }
}

fn is_event_device_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("event"))
}

fn open_event_device(path: &Path) -> Option<OwnedFd> {
    let file = OpenOptions::new()
        .desired_access(FileAccess::READ_DATA)
        .open(None, path)
        .ok()?;
    let fd: OwnedFd = file.into();
    set_cloexec_nonblocking(fd.as_raw_fd()).ok()?;
    Some(fd)
}
