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
/// inheritance then derives child SDs. Requires SeRestorePrivilege in our
/// token to replace the MISSING SD — held by the boot SYSTEM token.
///
/// Allow SYSTEM GenericAll and Allow BUILTIN\Administrators GenericAll, both
/// OI|CI, plus Allow Everyone read+execute, CI only. Because everything created
/// beneath these roots inherits from this one descriptor, it is the access
/// policy of `/run`, `/dev/shm` and the cgroup tree in their entirety; an ACE
/// missing here is missing from every per-service directory under
/// `/run/services`.
///
/// Administrators is present so that an administrator can enumerate those
/// trees at all — nothing bypasses `FILE_LIST_DIRECTORY`, so a SYSTEM-only
/// seed makes them invisible to any signed-on principal (PEI-223).
///
/// Everyone is present for the same reason one rung down: a service that is
/// not SYSTEM has to reach `/run` to find its own runtime directory. Execute
/// is the traverse bit (`KACS_FILE_TRAVERSE == KACS_FILE_EXECUTE`), and an
/// explicit `chdir` does not get the `SeChangeNotifyPrivilege` traverse
/// bypass, so read without execute would let a service list a directory and
/// still fail to enter it (PEI-546).
///
/// That ACE is `CI` and not `OI|CI`, which is the one place this descriptor
/// deliberately differs from the root's. What a service needs here is to walk
/// to its own directory, and container inheritance alone gives it that. Object
/// inheritance would additionally put Everyone-read on every *file* anything
/// creates under `/run` and `/dev/shm` — a service's runtime state, and shared
/// memory segments, world-readable by inheritance. The root needs `OI` because
/// a service has to read and execute the binaries beneath it; nothing here is
/// a binary.
///
/// SYSTEM and Administrators keep `GenericAll` rather than read/write/execute
/// because `GA` carries `WRITE_DAC`, `WRITE_OWNER` and `DELETE`, and peinit
/// re-stamps descriptors throughout `/run` — `bind_secured` and
/// `ensure_directory` both depend on it.
///
/// The `CREATOR OWNER` ACE (`S-1-3-0`, inherit-only) is what makes the `CI`
/// above survivable. A file created here inherits the SYSTEM and
/// Administrators ACEs and nothing else, and that is a *non-empty* DACL, so
/// `build_created_file_sd_bytes` never reaches the token's default DACL — a
/// service would create runtime state it could not read back. Resolved per
/// created object, and carried onward down each container, so a service owns
/// what it makes without anything else gaining a right to it.
///
/// Written as the literal SID because neither SDDL vocabulary has a `CO`
/// alias — the same reason `S-1-5-6` is spelled out on the boot path.
///
/// This MUST stay in step with the descriptor the live root's mount hook
/// stamps (`pkgs/live-boot/src/mount-root.sh`) and with peios-install's
/// `ROOT_SDDL` — identical but for the Everyone ACE's inheritance flags, for
/// the reason given above. Note what it is no longer tied to: `build_seed_sd` in
/// prelude's `seed-sd`, which stays narrower on purpose. That is the
/// *bootstrap* descriptor, and it is also what stamps `/dev`, where every ACE
/// is inherited by the next hot-plugged block device — a read ACE for
/// Everyone there is every filesystem ACL on the machine bypassed by opening
/// the raw disk. The three copies here are hand-copied; the last time they
/// drifted, Administrators was added in one and not the other, and the drift
/// went unnoticed for four days (PEI-223).
///
/// Unlike the live root's copy, this one is not a stopgap awaiting
/// package-shipped descriptors (PSPU §5.20): `/run`, `/dev/shm` and the
/// cgroup tree are created at runtime and belong to no package, so this stays
/// their policy.
const PHASE1_SEED_SDDL: &str = concat!(
    "O:SYG:SYD:(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)",
    "(A;CI;GRGX;;;WD)(A;OICIIO;GA;;;S-1-3-0)"
);

/// The synthesised descriptor for devpts inodes. devpts cannot store SDs
/// and its slave nodes are materialised by the kernel when a terminal opens
/// `/dev/ptmx` — never through a create path that could stamp or inherit
/// one — so under DENY_MISSING every pseudo-terminal on the machine is
/// unopenable (PEI-523 found it: Atrium's terminal died instantly). The
/// mount instead carries SYNTHESIZE_EPHEMERAL with this template.
///
/// Authenticated Users get read, write and traverse: enough to use a pty.
/// The template applies to every SD-less inode of the mount alike, so any
/// authenticated principal may open any slave — per-owner slave SDs need
/// kernel support for stamping the opener at materialisation; future work.
const DEVPTS_SYNTH_SDDL: &str = "O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FRFWFX;;;AU)";

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
    /// Set this KACS mount policy template (SYNTHESIZE_EPHEMERAL) on the
    /// mount after a successful mount — for filesystems that cannot store
    /// SDs and whose inodes appear outside any create path (devpts).
    synth_template: Option<&'static str>,
}

const PHASE1_VIRTUAL_MOUNTS: [Phase1VirtualMount; 7] = [
    Phase1VirtualMount {
        mount_point: "/proc",
        filesystem: "proc",
        flags: libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
        initramfs_provided: true,
        seed_after_mount: false,
        synth_template: None,
    },
    Phase1VirtualMount {
        mount_point: "/sys",
        filesystem: "sysfs",
        flags: libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
        initramfs_provided: true,
        seed_after_mount: false,
        synth_template: None,
    },
    Phase1VirtualMount {
        mount_point: "/dev",
        filesystem: "devtmpfs",
        flags: libc::MS_NOSUID,
        initramfs_provided: true,
        seed_after_mount: false,
        synth_template: None,
    },
    Phase1VirtualMount {
        mount_point: "/dev/pts",
        filesystem: "devpts",
        flags: libc::MS_NOSUID | libc::MS_NOEXEC,
        initramfs_provided: false,
        seed_after_mount: false,
        synth_template: Some(DEVPTS_SYNTH_SDDL),
    },
    Phase1VirtualMount {
        mount_point: "/dev/shm",
        filesystem: "tmpfs",
        flags: libc::MS_NOSUID | libc::MS_NODEV,
        initramfs_provided: false,
        seed_after_mount: true,
        synth_template: None,
    },
    Phase1VirtualMount {
        mount_point: "/run",
        filesystem: "tmpfs",
        flags: libc::MS_NOSUID | libc::MS_NODEV,
        initramfs_provided: false,
        seed_after_mount: true,
        synth_template: None,
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
        synth_template: None,
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
        if let Some(template) = spec.synth_template {
            syscalls
                .set_synth_policy(spec.mount_point, template)
                .map_err(|error| {
                    BoundaryError::Recovery(format!(
                        "set mount policy on {} failed: {error}",
                        spec.mount_point
                    ))
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
    /// Set a SYNTHESIZE_EPHEMERAL KACS mount policy with `template_sddl` on
    /// the mounted filesystem — for mounts that cannot hold SDs at all.
    fn set_synth_policy(&mut self, mount_point: &str, template_sddl: &str) -> io::Result<()>;
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

    fn set_synth_policy(&mut self, mount_point: &str, template_sddl: &str) -> io::Result<()> {
        use std::os::fd::{FromRawFd, OwnedFd};
        let sd = peios::security::sddl::parse(template_sddl).map_err(io::Error::from)?;
        // O_PATH: kacs_set_mount_policy resolves the superblock via
        // fget_raw, and an O_PATH open performs no KACS access check — the
        // mount is DENY_MISSING until this very call takes effect.
        let path = c_string(mount_point)?;
        // SAFETY: plain open(2); the fd is owned below.
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fresh fd from open.
        let file = peios::file::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        let policy = peios::file::MountPolicy {
            kind: peios::file::MountPolicyKind::SYNTHESIZE_EPHEMERAL,
            flags: 0,
            generation: 0,
            template_sd: Some(sd),
        };
        file.mount_set_policy(&policy).map_err(io::Error::from)
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
