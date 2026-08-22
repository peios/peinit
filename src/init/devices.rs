//! Device node policy: the per-node security descriptors peinit stamps on the
//! handful of static `/dev` nodes that every principal must be able to use.
//!
//! `/dev` arrives from the initramfs with one inheritable descriptor on the
//! root and on every node — SYSTEM and Administrators full control, nothing
//! for anyone else. That is the right *default*: whatever the root grants, a
//! disk hot-plugged later inherits, so the root must be something acceptable
//! on a raw block device nobody has met yet. But it makes `2>/dev/null` fail
//! for every ordinary user (PEI-208), and `/dev/null` is in more or less every
//! shell script ever written.
//!
//! A single inherited descriptor cannot say "`/dev/null` for everyone, the
//! disks for administrators". So the exceptions are enumerated: the nodes
//! below exist on every boot, never change meaning, and are stamped with their
//! own descriptor after the mounts are up. They are leaves — nothing inherits
//! from a character device — so re-stamping them later (when the registry is
//! available and `Machine\Software\Peinit\SdDefaults\<name>` can override the
//! compiled default) needs no special sequencing.
//!
//! This module is the policy and its application over a syscall trait; the
//! Linux `set_sd` lives behind the feature gate in `init::linux::devices`.
//! Keeping the logic here means the default test suite exercises it.

use std::io;

/// One static device node and the descriptor it gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceNodePolicy {
    /// The `SdDefaults` value name this descriptor will be read from once the
    /// registry override lands (PEI-224). Also the name used in messages.
    pub name: &'static str,
    /// The node, as an absolute path.
    pub path: &'static str,
    /// The DACL to stamp, as SDDL. DACL only: owner and group stay whatever
    /// the seed gave the node (SYSTEM), and the ACEs carry no inheritance
    /// flags because a device node has no children.
    pub sddl: &'static str,
}

/// Everyone may read and write, and nothing more.
///
/// `FR` | `FW` is READ_DATA, WRITE_DATA, APPEND_DATA, the EA and attribute
/// rights, READ_CONTROL and SYNCHRONIZE — what a program needs to open the
/// node for reading or writing and to stat it. It deliberately omits
/// WRITE_DAC and WRITE_OWNER: an ordinary user may use `/dev/null`, not
/// re-ACL it. SYSTEM and Administrators keep full control, as the seed gave
/// them.
const EVERYONE_READ_WRITE_SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FRFW;;;WD)";

/// The static nodes and their descriptors, in the order they are applied.
///
/// Deliberately absent: `console` (the SYSTEM console), `mem`, `kmsg`, and
/// every disk, tty and serial port — those keep the inherited default.
/// `stdin`, `stdout`, `stderr` and `fd` are not here because devtmpfs does
/// not create them; they are udev's work elsewhere and do not exist on Peios.
pub const DEVICE_NODE_POLICIES: &[DeviceNodePolicy] = &[
    DeviceNodePolicy {
        name: "DevNull",
        path: "/dev/null",
        sddl: EVERYONE_READ_WRITE_SDDL,
    },
    DeviceNodePolicy {
        name: "DevZero",
        path: "/dev/zero",
        sddl: EVERYONE_READ_WRITE_SDDL,
    },
    DeviceNodePolicy {
        name: "DevFull",
        path: "/dev/full",
        sddl: EVERYONE_READ_WRITE_SDDL,
    },
    DeviceNodePolicy {
        name: "DevRandom",
        path: "/dev/random",
        sddl: EVERYONE_READ_WRITE_SDDL,
    },
    DeviceNodePolicy {
        name: "DevUrandom",
        path: "/dev/urandom",
        sddl: EVERYONE_READ_WRITE_SDDL,
    },
    DeviceNodePolicy {
        name: "DevTty",
        path: "/dev/tty",
        sddl: EVERYONE_READ_WRITE_SDDL,
    },
    DeviceNodePolicy {
        name: "DevPtmx",
        path: "/dev/ptmx",
        sddl: EVERYONE_READ_WRITE_SDDL,
    },
];

/// What applying the policy did, node by node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceNodePolicyReport {
    /// Nodes whose descriptor was stamped.
    pub applied: Vec<String>,
    /// Nodes that do not exist on this system. Not a failure: a node that is
    /// absent grants nothing to anyone, so there is no exposure to correct.
    pub missing: Vec<String>,
    /// Nodes that exist but could not be stamped. Each stays on the inherited
    /// default — usable by administrators, denied to everyone else.
    pub failures: Vec<DeviceNodePolicyFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceNodePolicyFailure {
    pub name: String,
    pub path: String,
    pub message: String,
}

/// The one syscall the policy needs: replace a node's DACL with the given
/// SDDL. An implementation must report a non-existent node as
/// [`io::ErrorKind::NotFound`].
pub trait DeviceNodeSyscalls {
    fn set_dacl(&mut self, path: &str, sddl: &str) -> io::Result<()>;
}

/// Apply every entry of [`DEVICE_NODE_POLICIES`]. Best-effort: one node's
/// failure never stops the next, and the caller decides what the report
/// means — for peinit, a warning, never recovery.
pub fn apply_device_node_policy<S>(syscalls: &mut S) -> DeviceNodePolicyReport
where
    S: DeviceNodeSyscalls + ?Sized,
{
    apply_policies(DEVICE_NODE_POLICIES, syscalls)
}

fn apply_policies<S>(policies: &[DeviceNodePolicy], syscalls: &mut S) -> DeviceNodePolicyReport
where
    S: DeviceNodeSyscalls + ?Sized,
{
    let mut report = DeviceNodePolicyReport::default();
    for policy in policies {
        match syscalls.set_dacl(policy.path, policy.sddl) {
            Ok(()) => report.applied.push(policy.path.to_string()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                report.missing.push(policy.path.to_string());
            }
            Err(error) => report.failures.push(DeviceNodePolicyFailure {
                name: policy.name.to_string(),
                path: policy.path.to_string(),
                message: error.to_string(),
            }),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Default)]
    struct FakeSyscalls {
        calls: Vec<(String, String)>,
        failures: BTreeMap<String, i32>,
    }

    impl DeviceNodeSyscalls for FakeSyscalls {
        fn set_dacl(&mut self, path: &str, sddl: &str) -> io::Result<()> {
            self.calls.push((path.to_string(), sddl.to_string()));
            match self.failures.get(path) {
                Some(errno) => Err(io::Error::from_raw_os_error(*errno)),
                None => Ok(()),
            }
        }
    }

    #[test]
    fn stamps_every_policy_node_with_its_sddl() {
        let mut syscalls = FakeSyscalls::default();

        let report = apply_device_node_policy(&mut syscalls);

        let expected: Vec<(String, String)> = DEVICE_NODE_POLICIES
            .iter()
            .map(|p| (p.path.to_string(), p.sddl.to_string()))
            .collect();
        assert_eq!(syscalls.calls, expected);
        assert_eq!(
            report.applied,
            DEVICE_NODE_POLICIES
                .iter()
                .map(|p| p.path.to_string())
                .collect::<Vec<_>>()
        );
        assert!(report.missing.is_empty());
        assert!(report.failures.is_empty());
    }

    #[test]
    fn a_missing_node_is_reported_as_missing_not_failed() {
        let mut syscalls = FakeSyscalls::default();
        syscalls
            .failures
            .insert("/dev/full".to_string(), libc::ENOENT);

        let report = apply_device_node_policy(&mut syscalls);

        assert_eq!(report.missing, vec!["/dev/full".to_string()]);
        assert!(report.failures.is_empty());
        assert_eq!(report.applied.len(), DEVICE_NODE_POLICIES.len() - 1);
    }

    #[test]
    fn a_failure_on_one_node_does_not_stop_the_rest() {
        let mut syscalls = FakeSyscalls::default();
        syscalls
            .failures
            .insert("/dev/null".to_string(), libc::EACCES);

        let report = apply_device_node_policy(&mut syscalls);

        assert_eq!(syscalls.calls.len(), DEVICE_NODE_POLICIES.len());
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].name, "DevNull");
        assert_eq!(report.failures[0].path, "/dev/null");
        assert_eq!(report.applied.len(), DEVICE_NODE_POLICIES.len() - 1);
    }

    #[test]
    fn policy_grants_everyone_read_write_without_write_dac_or_owner() {
        // The whole point of the exception list: Everyone may use the node,
        // not re-ACL it. Guard the SDDL text so a "helpful" widening to GA
        // cannot slip through unnoticed.
        for policy in DEVICE_NODE_POLICIES {
            assert!(policy.sddl.contains("(A;;FRFW;;;WD)"), "{}", policy.path);
            assert!(!policy.sddl.contains("GA;;;WD"), "{}", policy.path);
            assert!(!policy.sddl.contains("OICI"), "{}: leaves carry no inheritance flags", policy.path);
        }
    }

    #[test]
    fn every_policy_node_lives_directly_under_dev() {
        for policy in DEVICE_NODE_POLICIES {
            let rest = policy.path.strip_prefix("/dev/").expect(policy.path);
            assert!(!rest.contains('/'), "{}", policy.path);
            assert!(policy.name.starts_with("Dev"), "{}", policy.name);
        }
    }
}
