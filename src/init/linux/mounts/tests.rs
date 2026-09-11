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
    SynthPolicy(String, String),
}

#[derive(Debug)]
struct FakeMountSyscalls {
    mountinfo: String,
    read_failures: VecDeque<i32>,
    calls: Vec<MountCall>,
    /// How many mount-side calls had been made when each mountinfo read
    /// happened, so a test can say where in the sequence a read fell.
    reads_at: Vec<usize>,
    mount_failures: BTreeMap<String, i32>,
    seed_failures: BTreeMap<String, i32>,
    policy_failures: BTreeMap<String, i32>,
}

impl FakeMountSyscalls {
    fn with_mountinfo(mountinfo: &str) -> Self {
        Self {
            mountinfo: mountinfo.to_string(),
            read_failures: VecDeque::new(),
            calls: Vec::new(),
            reads_at: Vec::new(),
            mount_failures: BTreeMap::new(),
            seed_failures: BTreeMap::new(),
            policy_failures: BTreeMap::new(),
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

    fn fail_policy(mut self, mount_point: &str, errno: i32) -> Self {
        self.policy_failures.insert(mount_point.to_string(), errno);
        self
    }
}

impl Phase1MountSyscalls for FakeMountSyscalls {
    fn read_mountinfo(&mut self, _path: &Path) -> std::io::Result<String> {
        self.reads_at.push(self.calls.len());
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

    fn set_synth_policy(&mut self, mount_point: &str, template_sddl: &str) -> std::io::Result<()> {
        self.calls.push(MountCall::SynthPolicy(
            mount_point.to_string(),
            template_sddl.to_string(),
        ));
        if let Some(errno) = self.policy_failures.get(mount_point) {
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

/// TRM §2.3 step 2: mountinfo lives in `/proc`, which is one of the things
/// being checked for, so a read that fails with `ENOENT` *or* `ENOTDIR` mounts
/// `/proc` from the table and reads again. The second read is a real retry: it
/// happens after the `/proc` mount and its result is what the rest of the step
/// works from, so `/proc` is not mounted a second time.
#[test]
fn mountinfo_enoent_or_enotdir_mounts_proc_and_reads_again() {
    for errno in [libc::ENOENT, libc::ENOTDIR] {
        let mountinfo_after_proc = "1 0 0:1 / /proc rw - proc proc rw\n";
        let mut syscalls =
            FakeMountSyscalls::with_mountinfo(mountinfo_after_proc).fail_read_once(errno);

        mount_phase1_virtual_filesystems(Path::new(DEFAULT_MOUNTINFO_PATH), &mut syscalls)
            .unwrap_or_else(|error| panic!("errno {errno}: mount setup failed: {error:?}"));

        assert_eq!(
            syscalls.reads_at,
            vec![0, 2],
            "errno {errno}: one failed read, then a second read after /proc's create and mount",
        );
        assert_eq!(
            syscalls.calls[..2],
            [
                MountCall::CreateDir("/proc".to_string()),
                MountCall::Mount {
                    mount_point: "/proc".to_string(),
                    filesystem: "proc".to_string(),
                    flags: libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
                },
            ],
            "errno {errno}: /proc is mounted before the retry",
        );
        let proc_mounts = syscalls
            .calls
            .iter()
            .filter(|call| matches!(call, MountCall::Mount { mount_point, .. } if mount_point == "/proc"))
            .count();
        assert_eq!(
            proc_mounts, 1,
            "errno {errno}: the retried mountinfo lists /proc, so it is not mounted again",
        );
    }
}

/// TRM §2.3 step 2: any failure to read or parse mountinfo other than the
/// `ENOENT`/`ENOTDIR` bootstrap case is a recovery error — including the retry
/// itself failing — and nothing further is mounted on the strength of a table
/// peinit could not read.
#[test]
fn an_unreadable_or_unparseable_mountinfo_is_a_recovery_error() {
    // Any other read errno: no retry, no /proc mount, no mounts at all.
    let mut syscalls = FakeMountSyscalls::with_mountinfo("").fail_read_once(libc::EACCES);
    let error = mount_phase1_virtual_filesystems(Path::new(DEFAULT_MOUNTINFO_PATH), &mut syscalls)
        .expect_err("an EACCES mountinfo read is fatal");
    assert!(
        matches!(&error, crate::boundary::BoundaryError::Recovery(message)
            if message.contains("read /proc/self/mountinfo failed")),
        "{error:?}",
    );
    assert_eq!(syscalls.reads_at, vec![0], "read once, not retried");
    assert!(syscalls.calls.is_empty(), "nothing mounted: {:?}", syscalls.calls);

    // The bootstrap retry that fails too: /proc was mounted for it, and then
    // the step stops.
    let mut syscalls = FakeMountSyscalls::with_mountinfo("")
        .fail_read_once(libc::ENOENT)
        .fail_read_once(libc::EIO);
    let error = mount_phase1_virtual_filesystems(Path::new(DEFAULT_MOUNTINFO_PATH), &mut syscalls)
        .expect_err("a failed retry is fatal");
    assert!(
        matches!(&error, crate::boundary::BoundaryError::Recovery(message)
            if message.contains("read /proc/self/mountinfo failed")),
        "{error:?}",
    );
    assert_eq!(syscalls.reads_at, vec![0, 2]);
    assert_eq!(
        syscalls.calls.len(),
        2,
        "only /proc's create and mount, for the retry: {:?}",
        syscalls.calls,
    );

    // Readable but not parseable.
    let mut syscalls = FakeMountSyscalls::with_mountinfo("26 23 0:22 /\n");
    let error = mount_phase1_virtual_filesystems(Path::new(DEFAULT_MOUNTINFO_PATH), &mut syscalls)
        .expect_err("an unparseable mountinfo is fatal");
    assert!(
        matches!(&error, crate::boundary::BoundaryError::Recovery(message)
            if message.contains("parse /proc/self/mountinfo failed")),
        "{error:?}",
    );
    assert!(syscalls.calls.is_empty(), "nothing mounted: {:?}", syscalls.calls);
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
    if let Some(template) = spec.synth_template {
        calls.push(MountCall::SynthPolicy(
            spec.mount_point.to_string(),
            template.to_string(),
        ));
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
fn sets_the_synth_policy_on_devpts_alone() {
    let mut syscalls = FakeMountSyscalls::with_mountinfo("");
    mount_phase1_virtual_filesystems(Path::new(DEFAULT_MOUNTINFO_PATH), &mut syscalls)
        .expect("mounts");
    let policies: Vec<_> = syscalls
        .calls
        .iter()
        .filter(|c| matches!(c, MountCall::SynthPolicy(..)))
        .collect();
    assert_eq!(policies.len(), 1);
    let MountCall::SynthPolicy(mount_point, template) = policies[0] else {
        unreachable!()
    };
    assert_eq!(mount_point, "/dev/pts");
    // Authenticated Users must be able to use a pty; SYSTEM and
    // Administrators keep full control.
    assert!(template.contains(";;;AU)"), "{template}");
    assert!(template.contains("(A;;GA;;;SY)"), "{template}");
}

#[test]
fn surfaces_policy_failure_as_recovery_error() {
    let mut syscalls = FakeMountSyscalls::with_mountinfo("").fail_policy("/dev/pts", libc::EACCES);
    let error = mount_phase1_virtual_filesystems(Path::new(DEFAULT_MOUNTINFO_PATH), &mut syscalls)
        .expect_err("policy failure");
    assert!(format!("{error:?}").contains("set mount policy on /dev/pts"));
}

#[test]
fn surfaces_seed_failure_as_recovery_error() {
    let mut syscalls = FakeMountSyscalls::with_mountinfo("").fail_seed("/run", libc::EACCES);

    let error = mount_phase1_virtual_filesystems(Path::new("/mountinfo"), &mut syscalls)
        .expect_err("seed failure");

    assert!(format!("{error:?}").contains("seed SD on /run failed"));
}
