use crate::init::Phase1JfsDevice;
use crate::runtime::{RuntimeEventRegistrar, RuntimeEventRegistrationError, RuntimeEventSource};

#[derive(Debug)]
#[cfg_attr(not(feature = "peios-boundary"), allow(dead_code))]
pub(crate) struct RuntimeJfsDevice {
    _device: Phase1JfsDevice,
    fd: i32,
}

impl RuntimeJfsDevice {
    #[cfg_attr(not(feature = "peios-boundary"), allow(dead_code))]
    pub(crate) fn fd(&self) -> i32 {
        self.fd
    }
}

#[cfg_attr(not(feature = "peios-boundary"), allow(dead_code))]
pub(crate) fn register_phase1_jfs_device<R>(
    device: Phase1JfsDevice,
    registrar: &mut R,
) -> Result<RuntimeJfsDevice, RuntimeEventRegistrationError>
where
    R: RuntimeEventRegistrar + ?Sized,
{
    let fd = device.as_raw_fd();
    registrar.register_source(fd, RuntimeEventSource::JfsDevice { fd })?;
    Ok(RuntimeJfsDevice {
        _device: device,
        fd,
    })
}

#[cfg(test)]
mod tests {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    use crate::init::Phase1JfsDevice;
    use crate::runtime::{
        RuntimeEventRegistrar, RuntimeEventRegistrationError, RuntimeEventSource,
    };

    use super::register_phase1_jfs_device;

    #[test]
    fn registers_phase1_jfs_fd_as_runtime_source() {
        let fd = pipe_read_fd();
        let raw_fd = fd.as_raw_fd();
        let device = Phase1JfsDevice::new(fd, "/dev/jfs");
        let mut registrar = FakeRegistrar::default();

        let registered = register_phase1_jfs_device(device, &mut registrar).expect("register jfs");

        assert_eq!(registered.fd(), raw_fd);
        assert_eq!(
            registrar.calls,
            vec![(raw_fd, RuntimeEventSource::JfsDevice { fd: raw_fd })],
        );
    }

    #[test]
    fn registration_failure_returns_error_and_drops_fd() {
        let fd = pipe_read_fd();
        let raw_fd = fd.as_raw_fd();
        let device = Phase1JfsDevice::new(fd, "/dev/jfs");
        let mut registrar = FakeRegistrar::failing();

        let error =
            register_phase1_jfs_device(device, &mut registrar).expect_err("registration failure");

        assert!(matches!(
            error,
            RuntimeEventRegistrationError::Register { fd, .. } if fd == raw_fd
        ));
    }

    #[derive(Debug, Default)]
    struct FakeRegistrar {
        calls: Vec<(i32, RuntimeEventSource)>,
        fail: bool,
    }

    impl FakeRegistrar {
        fn failing() -> Self {
            Self {
                fail: true,
                ..Self::default()
            }
        }
    }

    impl RuntimeEventRegistrar for FakeRegistrar {
        fn register_source(
            &mut self,
            fd: i32,
            source: RuntimeEventSource,
        ) -> Result<(), RuntimeEventRegistrationError> {
            self.calls.push((fd, source));
            if self.fail {
                Err(RuntimeEventRegistrationError::Register {
                    fd,
                    source,
                    message: "register failed".to_string(),
                })
            } else {
                Ok(())
            }
        }

        fn unregister_source(&mut self, _fd: i32) -> Result<(), RuntimeEventRegistrationError> {
            Ok(())
        }
    }

    fn pipe_read_fd() -> OwnedFd {
        let mut fds = [0_i32; 2];
        let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
        assert_eq!(
            result,
            0,
            "pipe2 failed: {}",
            std::io::Error::last_os_error()
        );
        unsafe {
            libc::close(fds[1]);
            OwnedFd::from_raw_fd(fds[0])
        }
    }
}
