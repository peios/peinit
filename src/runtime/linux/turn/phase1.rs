use crate::init::Phase1Infrastructure;

use crate::runtime::jfs::register_phase1_jfs_device;
use crate::runtime::linux::{
    LinuxShutdownRuntime, Phase1InfrastructureRegistration, Phase1JfsRegistration,
};

impl LinuxShutdownRuntime {
    pub fn register_phase1_infrastructure(
        &mut self,
        infrastructure: &mut Phase1Infrastructure,
    ) -> Phase1InfrastructureRegistration {
        let jfs = match infrastructure.take_jfs_device() {
            Some(device) => {
                let fd = device.as_raw_fd();
                let path = device.path().to_string();
                match register_phase1_jfs_device(device, &mut self.epoll) {
                    Ok(registered) => {
                        self.jfs_device = Some(registered);
                        Phase1JfsRegistration::Registered { fd, path }
                    }
                    Err(error) => Phase1JfsRegistration::RegisterFailed { fd, path, error },
                }
            }
            None => Phase1JfsRegistration::Unavailable,
        };
        Phase1InfrastructureRegistration { jfs }
    }
}
