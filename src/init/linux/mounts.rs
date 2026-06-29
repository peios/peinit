use std::ffi::CString;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

use peios::file::{FileAccess, OpenOptions, SecInfo};

use crate::boundary::{BoundaryError, read_fd_to_string};

pub(super) const DEFAULT_MOUNTINFO_PATH: &str = "/proc/self/mountinfo";

/// The SYSTEM-owned, container+object-inheritable default SD seeded onto each
/// fresh tmpfs root we mount. A new tmpfs carries no SD, so KACS DENY_MISSING
/// locks every inode on it (mkdir/touch → EACCES) until this is stamped;
/// inheritance then derives child SDs. Same descriptor prelude's seed-sd helper
/// writes (Allow SYSTEM GENERIC_ALL, OI|CI). Requires SeRestorePrivilege in our
/// token to replace the MISSING SD — held by the boot SYSTEM token.
const PHASE1_SEED_SDDL: &str = "O:SYG:SYD:(A;OICI;GA;;;SY)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Phase1VirtualMount {
    pub mount_point: &'static str,
    pub filesystem: &'static str,
    pub flags: libc::c_ulong,
    initramfs_provided: bool,
    /// Seed [`PHASE1_SEED_SDDL`] onto this mount root after a successful mount.
    /// Set for the fresh managed roots we create that are SD-less at mount
    /// (tmpfs + cgroup2 — all DENY_MISSING); NOT for proc/sysfs (UNMANAGED, no
    /// SD) or initramfs-provided mounts (already up and seeded).
    seed_after_mount: bool,
}

const PHASE1_VIRTUAL_MOUNTS: [Phase1VirtualMount; 7] = [
    Phase1VirtualMount {
        mount_point: "/proc",
        filesystem: "proc",
        flags: libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
        initramfs_provided: true,
        seed_after_mount: false,
    },
    Phase1VirtualMount {
        mount_point: "/sys",
        filesystem: "sysfs",
        flags: libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
        initramfs_provided: true,
        seed_after_mount: false,
    },
    Phase1VirtualMount {
        mount_point: "/dev",
        filesystem: "devtmpfs",
        flags: libc::MS_NOSUID,
        initramfs_provided: true,
        seed_after_mount: false,
    },
    Phase1VirtualMount {
        mount_point: "/dev/pts",
        filesystem: "devpts",
        flags: libc::MS_NOSUID | libc::MS_NOEXEC,
        initramfs_provided: false,
        seed_after_mount: false,
    },
    Phase1VirtualMount {
        mount_point: "/dev/shm",
        filesystem: "tmpfs",
        flags: libc::MS_NOSUID | libc::MS_NODEV,
        initramfs_provided: false,
        seed_after_mount: true,
    },
    Phase1VirtualMount {
        mount_point: "/run",
        filesystem: "tmpfs",
        flags: libc::MS_NOSUID | libc::MS_NODEV,
        initramfs_provided: false,
        seed_after_mount: true,
    },
    Phase1VirtualMount {
        mount_point: "/sys/fs/cgroup",
        filesystem: "cgroup2",
        flags: libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
        initramfs_provided: false,
        // cgroup2 is DENY_MISSING like any managed fs, and peinit manages the
        // service cgroup tree through KACS-native directory creation (each new
        // cgroup dir inherits an SD). Seed the root so that inheritance has a
        // parent SD and the tree is creatable; without it the first
        // `/sys/fs/cgroup` open is DENY_MISSING-locked (EACCES). cgroupfs
        // (kernfs) stores the SD in a security xattr.
        seed_after_mount: true,
    },
];

pub(super) fn mount_phase1_virtual_filesystems<S>(
    mountinfo_path: &Path,
    syscalls: &mut S,
) -> Result<(), BoundaryError>
where
    S: Phase1MountSyscalls + ?Sized,
{
    let mountinfo = read_phase1_mountinfo(mountinfo_path, syscalls)?;
    let mounted = crate::mountinfo::parse_mountinfo_mount_points(&mountinfo).map_err(|error| {
        BoundaryError::Recovery(format!(
            "parse {} failed: {error}",
            mountinfo_path.display()
        ))
    })?;

    for spec in PHASE1_VIRTUAL_MOUNTS {
        if mounted
            .iter()
            .any(|mount_point| mount_point == spec.mount_point)
        {
            continue;
        }
        mount_phase1_spec(syscalls, spec)?;
        if spec.seed_after_mount {
            syscalls.seed_sd(spec.mount_point).map_err(|error| {
                BoundaryError::Recovery(format!("seed SD on {} failed: {error}", spec.mount_point))
            })?;
        }
    }

    Ok(())
}

fn read_phase1_mountinfo<S>(
    mountinfo_path: &Path,
    syscalls: &mut S,
) -> Result<String, BoundaryError>
where
    S: Phase1MountSyscalls + ?Sized,
{
    match syscalls.read_mountinfo(mountinfo_path) {
        Ok(mountinfo) => Ok(mountinfo),
        Err(error) if should_mount_proc_before_mountinfo_read(mountinfo_path, &error) => {
            mount_phase1_spec(syscalls, PHASE1_VIRTUAL_MOUNTS[0])?;
            syscalls
                .read_mountinfo(mountinfo_path)
                .map_err(|error| mountinfo_read_error(mountinfo_path, error))
        }
        Err(error) => Err(mountinfo_read_error(mountinfo_path, error)),
    }
}

fn should_mount_proc_before_mountinfo_read(mountinfo_path: &Path, error: &io::Error) -> bool {
    mountinfo_path == Path::new(DEFAULT_MOUNTINFO_PATH)
        && (is_errno(error, libc::ENOENT) || is_errno(error, libc::ENOTDIR))
}

fn mountinfo_read_error(mountinfo_path: &Path, error: io::Error) -> BoundaryError {
    BoundaryError::Recovery(format!("read {} failed: {error}", mountinfo_path.display()))
}

fn mount_phase1_spec<S>(syscalls: &mut S, spec: Phase1VirtualMount) -> Result<(), BoundaryError>
where
    S: Phase1MountSyscalls + ?Sized,
{
    syscalls.create_dir_all(spec.mount_point).map_err(|error| {
        BoundaryError::Recovery(format!(
            "create mount point {} failed: {error}",
            spec.mount_point
        ))
    })?;
    match syscalls.mount(spec) {
        Ok(()) => Ok(()),
        Err(error) if spec.initramfs_provided && is_errno(&error, libc::EBUSY) => Ok(()),
        Err(error) => Err(BoundaryError::Recovery(format!(
            "mount {} as {} failed: {error}",
            spec.mount_point, spec.filesystem
        ))),
    }
}

pub(super) trait Phase1MountSyscalls {
    fn read_mountinfo(&mut self, path: &Path) -> io::Result<String>;
    fn create_dir_all(&mut self, mount_point: &str) -> io::Result<()>;
    fn mount(&mut self, spec: Phase1VirtualMount) -> io::Result<()>;
    /// Stamp [`PHASE1_SEED_SDDL`] (owner + group + DACL) onto a freshly mounted
    /// filesystem root so KACS stops DENY_MISSING-locking its inodes.
    fn seed_sd(&mut self, mount_point: &str) -> io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LinuxPhase1MountSyscalls;

impl Phase1MountSyscalls for LinuxPhase1MountSyscalls {
    fn read_mountinfo(&mut self, path: &Path) -> io::Result<String> {
        let file = OpenOptions::new()
            .desired_access(FileAccess::READ_DATA)
            .open(None, path)
            .map_err(io::Error::from)?;
        let fd = file.as_raw_fd();
        read_fd_to_string(fd)
    }

    fn create_dir_all(&mut self, mount_point: &str) -> io::Result<()> {
        std::fs::create_dir_all(mount_point)
    }

    fn mount(&mut self, spec: Phase1VirtualMount) -> io::Result<()> {
        mount_virtual_filesystem(spec)
    }

    fn seed_sd(&mut self, mount_point: &str) -> io::Result<()> {
        let sd = peios::security::sddl::parse(PHASE1_SEED_SDDL).map_err(io::Error::from)?;
        let info = SecInfo::OWNER | SecInfo::GROUP | SecInfo::DACL;
        peios::file::set_sd(None, Path::new(mount_point), info, &sd, 0).map_err(io::Error::from)
    }
}

fn mount_virtual_filesystem(spec: Phase1VirtualMount) -> io::Result<()> {
    let source = c_string(spec.filesystem)?;
    let target = c_string(spec.mount_point)?;
    let filesystem = c_string(spec.filesystem)?;
    let rc = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            filesystem.as_ptr(),
            spec.flags,
            std::ptr::null(),
        )
    };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn c_string(value: &str) -> io::Result<CString> {
    CString::new(value).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

fn is_errno(error: &io::Error, errno: i32) -> bool {
    error.raw_os_error() == Some(errno)
}

#[cfg(test)]
mod tests;
