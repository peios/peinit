use std::io;
use std::path::Path;

use peios::file::SecInfo;

use crate::init::devices::{DeviceNodePolicyReport, DeviceNodeSyscalls, apply_device_node_policy};

/// Apply the compiled device node policy to the live `/dev`.
pub(super) fn apply_linux_device_node_policy() -> DeviceNodePolicyReport {
    let mut syscalls = LinuxDeviceNodeSyscalls;
    apply_device_node_policy(&mut syscalls)
}

#[derive(Debug, Clone, Copy, Default)]
struct LinuxDeviceNodeSyscalls;

impl DeviceNodeSyscalls for LinuxDeviceNodeSyscalls {
    /// DACL only — the seed already gave the node its owner and group, and
    /// the policy has no opinion about them. The node is a leaf, so the
    /// symlink question does not arise: none of the policy paths is a link.
    fn set_dacl(&mut self, path: &str, sddl: &str) -> io::Result<()> {
        let sd = peios::security::sddl::parse(sddl).map_err(io::Error::from)?;
        peios::file::set_sd(None, Path::new(path), SecInfo::DACL, &sd, 0).map_err(io::Error::from)
    }
}
