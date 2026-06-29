use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use super::{
    DEFAULT_MOUNTINFO_PATH, PHASE1_VIRTUAL_MOUNTS, Phase1MountSyscalls, Phase1VirtualMount,
    mount_phase1_virtual_filesystems,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum MountCall {
    CreateDir(String),
    Mount {
        mount_point: String,
        filesystem: String,
        flags: libc::c_ulong,
    },
    Seed(String),
}

#[derive(Debug)]
struct FakeMountSyscalls {
    mountinfo: String,
    read_failures: VecDeque<i32>,
    calls: Vec<MountCall>,
    mount_failures: BTreeMap<String, i32>,
    seed_failures: BTreeMap<String, i32>,
}

impl FakeMountSyscalls {
    fn with_mountinfo(mountinfo: &str) -> Self {
        Self {
            mountinfo: mountinfo.to_string(),
            read_failures: VecDeque::new(),
            calls: Vec::new(),
            mount_failures: BTreeMap::new(),
            seed_failures: BTreeMap::new(),
        }
    }

    fn fail_read_once(mut self, errno: i32) -> Self {
        self.read_failures.push_back(errno);
        self
    }

    fn fail_mount(mut self, mount_point: &str, errno: i32) -> Self {
        self.mount_failures.insert(mount_point.to_string(), errno);
        self
    }

    fn fail_seed(mut self, mount_point: &str, errno: i32) -> Self {
        self.seed_failures.insert(mount_point.to_string(), errno);
        self
    }
}

impl Phase1MountSyscalls for FakeMountSyscalls {
    fn read_mountinfo(&mut self, _path: &Path) -> std::io::Result<String> {
        if let Some(errno) = self.read_failures.pop_front() {
            return Err(std::io::Error::from_raw_os_error(errno));
        }
        Ok(self.mountinfo.clone())
    }

    fn create_dir_all(&mut self, mount_point: &str) -> std::io::Result<()> {
        self.calls
            .push(MountCall::CreateDir(mount_point.to_string()));
        Ok(())
    }

    fn mount(&mut self, spec: Phase1VirtualMount) -> std::io::Result<()> {
        self.calls.push(MountCall::Mount {
            mount_point: spec.mount_point.to_string(),
            filesystem: spec.filesystem.to_string(),
            flags: spec.flags,
        });
        if let Some(errno) = self.mount_failures.get(spec.mount_point) {
            Err(std::io::Error::from_raw_os_error(*errno))
        } else {
            Ok(())
        }
    }

    fn seed_sd(&mut self, mount_point: &str) -> std::io::Result<()> {
        self.calls.push(MountCall::Seed(mount_point.to_string()));
        if let Some(errno) = self.seed_failures.get(mount_point) {
            Err(std::io::Error::from_raw_os_error(*errno))
        } else {
            Ok(())
        }
    }
}

#[test]
fn skips_mounts_already_present_in_mountinfo() {
    let mountinfo = "\
1 0 0:1 / /proc rw - proc proc rw
2 0 0:2 / /sys rw - sysfs sysfs rw
3 0 0:3 / /dev rw - devtmpfs devtmpfs rw
4 0 0:4 / /dev/pts rw - devpts devpts rw
5 0 0:5 / /dev/shm rw - tmpfs tmpfs rw
6 0 0:6 / /run rw - tmpfs tmpfs rw
7 0 0:7 / /sys/fs/cgroup rw - cgroup2 cgroup2 rw
";
    let mut syscalls = FakeMountSyscalls::with_mountinfo(mountinfo);

    mount_phase1_virtual_filesystems(Path::new("/mountinfo"), &mut syscalls).expect("mount setup");

    assert!(syscalls.calls.is_empty());
}

#[test]
fn mounts_absent_filesystems_in_spec_order_with_exact_flags() {
    let mut syscalls = FakeMountSyscalls::with_mountinfo("");

    mount_phase1_virtual_filesystems(Path::new("/mountinfo"), &mut syscalls).expect("mount setup");

    let expected: Vec<MountCall> = PHASE1_VIRTUAL_MOUNTS
        .iter()
        .flat_map(expected_calls_for_spec)
        .collect();
    assert_eq!(syscalls.calls, expected);
}

#[test]
fn mounts_proc_before_reading_default_mountinfo_when_proc_is_absent() {
    let mountinfo_after_proc = "1 0 0:1 / /proc rw - proc proc rw\n";
    let mut syscalls =
        FakeMountSyscalls::with_mountinfo(mountinfo_after_proc).fail_read_once(libc::ENOENT);

    mount_phase1_virtual_filesystems(Path::new(DEFAULT_MOUNTINFO_PATH), &mut syscalls)
        .expect("mount setup");

    let expected: Vec<MountCall> = std::iter::once(&PHASE1_VIRTUAL_MOUNTS[0])
        .chain(PHASE1_VIRTUAL_MOUNTS[1..].iter())
        .flat_map(expected_calls_for_spec)
        .collect();
    assert_eq!(syscalls.calls, expected);
}

/// The CreateDir + Mount calls a spec produces, plus the Seed call for the
/// tmpfs roots that carry `seed_after_mount`.
fn expected_calls_for_spec(spec: &Phase1VirtualMount) -> Vec<MountCall> {
    let mut calls = vec![
        MountCall::CreateDir(spec.mount_point.to_string()),
        MountCall::Mount {
            mount_point: spec.mount_point.to_string(),
            filesystem: spec.filesystem.to_string(),
            flags: spec.flags,
        },
    ];
    if spec.seed_after_mount {
        calls.push(MountCall::Seed(spec.mount_point.to_string()));
    }
    calls
}

#[test]
fn treats_ebusy_as_success_for_initramfs_provided_mounts() {
    let mut syscalls = FakeMountSyscalls::with_mountinfo("").fail_mount("/proc", libc::EBUSY);

    mount_phase1_virtual_filesystems(Path::new("/mountinfo"), &mut syscalls).expect("mount setup");

    assert!(syscalls.calls.contains(&MountCall::Mount {
        mount_point: "/proc".to_string(),
        filesystem: "proc".to_string(),
        flags: libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
    }));
}

#[test]
fn rejects_peinit_owned_mount_failure() {
    let mut syscalls = FakeMountSyscalls::with_mountinfo("").fail_mount("/run", libc::EPERM);

    let error = mount_phase1_virtual_filesystems(Path::new("/mountinfo"), &mut syscalls)
        .expect_err("mount failure");

    assert!(format!("{error:?}").contains("mount /run as tmpfs failed"));
}

#[test]
fn seeds_fresh_managed_roots_after_mounting_them() {
    let mut syscalls = FakeMountSyscalls::with_mountinfo("");

    mount_phase1_virtual_filesystems(Path::new("/mountinfo"), &mut syscalls).expect("mount setup");

    // The fresh SD-less managed roots (tmpfs + cgroup2) are seeded; the
    // UNMANAGED virtual filesystems (proc/sysfs) and devpts are not.
    assert!(
        syscalls
            .calls
            .contains(&MountCall::Seed("/run".to_string()))
    );
    assert!(
        syscalls
            .calls
            .contains(&MountCall::Seed("/dev/shm".to_string()))
    );
    assert!(
        syscalls
            .calls
            .contains(&MountCall::Seed("/sys/fs/cgroup".to_string()))
    );
    assert!(
        !syscalls
            .calls
            .contains(&MountCall::Seed("/proc".to_string()))
    );
    assert!(
        !syscalls
            .calls
            .contains(&MountCall::Seed("/dev/pts".to_string()))
    );
}

#[test]
fn surfaces_seed_failure_as_recovery_error() {
    let mut syscalls = FakeMountSyscalls::with_mountinfo("").fail_seed("/run", libc::EACCES);

    let error = mount_phase1_virtual_filesystems(Path::new("/mountinfo"), &mut syscalls)
        .expect_err("seed failure");

    assert!(format!("{error:?}").contains("seed SD on /run failed"));
}
