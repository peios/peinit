use std::io;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::path::Path;

use peios::file::{FileAccess, OpenOptions};

use crate::boundary::BoundaryError;
use crate::control::socket::{CONTROL_SOCKET_PATH, LinuxControlSocket};
use crate::init::{Phase1Infrastructure, Phase1InfrastructureWarning, Phase1JfsDevice};

use super::loopback::{LinuxLoopbackNetlink, bring_up_loopback};

const DEFAULT_JFS_DEVICE_PATH: &str = "/dev/jfs";

pub(super) fn setup_linux_phase1_infrastructure() -> Result<Phase1Infrastructure, BoundaryError> {
    let control_socket = bind_control_socket(Path::new(CONTROL_SOCKET_PATH))?;
    let mut opener = LinuxJfsDeviceOpener;
    let mut infrastructure =
        setup_phase1_infrastructure_with_opener(Path::new(DEFAULT_JFS_DEVICE_PATH), &mut opener);
    infrastructure.set_control_socket(control_socket);
    let mut loopback = LinuxLoopbackNetlink;
    if let Err(error) = bring_up_loopback(&mut loopback) {
        infrastructure.push_warning(Phase1InfrastructureWarning::LoopbackBringUp {
            interface: "lo".to_string(),
            message: format!("{error:?}"),
        });
    }
    Ok(infrastructure)
}

fn setup_phase1_infrastructure_with_opener<O>(
    jfs_path: &Path,
    opener: &mut O,
) -> Phase1Infrastructure
where
    O: JfsDeviceOpener + ?Sized,
{
    let mut infrastructure = Phase1Infrastructure::new();
    match opener.open_jfs_device(jfs_path) {
        Ok(fd) => {
            infrastructure = Phase1Infrastructure::with_jfs_device(Phase1JfsDevice::new(
                fd,
                jfs_path.display().to_string(),
            ));
        }
        Err(error) => infrastructure.push_warning(Phase1InfrastructureWarning::JfsDeviceOpen {
            path: jfs_path.display().to_string(),
            message: error.to_string(),
        }),
    }
    infrastructure
}

fn bind_control_socket(path: &Path) -> Result<LinuxControlSocket, BoundaryError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            BoundaryError::Recovery(format!(
                "create control socket dir {} failed: {error}",
                parent.display()
            ))
        })?;
    }
    LinuxControlSocket::bind(path).map_err(|error| {
        BoundaryError::Recovery(format!(
            "bind control socket {} failed: {error:?}",
            path.display()
        ))
    })
}

trait JfsDeviceOpener {
    fn open_jfs_device(&mut self, path: &Path) -> io::Result<OwnedFd>;
}

struct LinuxJfsDeviceOpener;

impl JfsDeviceOpener for LinuxJfsDeviceOpener {
    fn open_jfs_device(&mut self, path: &Path) -> io::Result<OwnedFd> {
        let file = OpenOptions::new()
            .desired_access(FileAccess::READ_DATA | FileAccess::WRITE_DATA)
            .open(None, path)
            .map_err(io::Error::from)?;
        let fd = unsafe { OwnedFd::from_raw_fd(file.into_raw_fd()) };
        // Held for the whole runtime and never closed in the child, so without
        // CLOEXEC the ad-hoc job submission channel reaches every service and
        // the recovery shell. peinit TRM §5.4 names this fd specifically.
        crate::boundary::set_cloexec(fd.as_raw_fd())?;
        Ok(fd)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::path::Path;

    use super::{JfsDeviceOpener, setup_phase1_infrastructure_with_opener};

    #[test]
    fn available_jfs_device_is_retained() {
        let mut opener = FakeJfsDeviceOpener::new();

        let infrastructure =
            setup_phase1_infrastructure_with_opener(Path::new("/dev/jfs"), &mut opener);

        let jfs = infrastructure.jfs_device().expect("retained jfs fd");
        assert_eq!(jfs.path(), "/dev/jfs");
        assert_eq!(infrastructure.warnings(), []);
    }

    #[test]
    fn missing_jfs_device_is_a_warning_not_failure() {
        let mut opener = FakeJfsDeviceOpener::new().fail_open("/dev/jfs", libc::ENOENT);

        let infrastructure =
            setup_phase1_infrastructure_with_opener(Path::new("/dev/jfs"), &mut opener);

        assert!(infrastructure.jfs_device().is_none());
        assert_eq!(infrastructure.warnings().len(), 1);
    }

    #[derive(Debug, Default)]
    struct FakeJfsDeviceOpener {
        open_failures: BTreeMap<String, i32>,
    }

    impl FakeJfsDeviceOpener {
        fn new() -> Self {
            Self::default()
        }

        fn fail_open(mut self, path: &str, errno: i32) -> Self {
            self.open_failures.insert(path.to_string(), errno);
            self
        }
    }

    impl JfsDeviceOpener for FakeJfsDeviceOpener {
        fn open_jfs_device(&mut self, path: &Path) -> io::Result<OwnedFd> {
            if let Some(errno) = self.open_failures.get(&path.display().to_string()) {
                return Err(io::Error::from_raw_os_error(*errno));
            }
            let mut fds = [0_i32; 2];
            let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
            assert_eq!(result, 0, "pipe2 failed: {}", io::Error::last_os_error());
            unsafe {
                libc::close(fds[1]);
                Ok(OwnedFd::from_raw_fd(fds[0]))
            }
        }
    }
}
