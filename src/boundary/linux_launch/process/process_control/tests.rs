use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use crate::boundary::ProcessPreExecStep;

use super::super::model::{ChildSetupEvidence, ChildSetupStatus, MalformedChildSetupEvidence};
use super::read_child_setup_status;

const SETUP_TIMEOUT_SECS: u64 = 1;

#[test]
fn child_setup_status_reads_exec_success_from_eof() {
    let (read, write) = pipe();
    drop(write);

    assert_eq!(
        read_child_setup_status(&read, SETUP_TIMEOUT_SECS).expect("status"),
        ChildSetupStatus::ExecSucceeded,
    );
}

#[test]
fn child_setup_status_reads_structured_failure() {
    let (read, write) = pipe();
    let evidence = ChildSetupEvidence {
        step: ProcessPreExecStep::Exec,
        errno: libc::ENOENT,
    };
    write_all(&write, &evidence.encode());
    drop(write);

    assert_eq!(
        read_child_setup_status(&read, SETUP_TIMEOUT_SECS).expect("status"),
        ChildSetupStatus::SetupFailed(evidence),
    );
}

#[test]
fn child_setup_status_rejects_malformed_length() {
    let (read, write) = pipe();
    write_all(&write, &[1, 2, 3]);
    drop(write);

    assert_eq!(
        read_child_setup_status(&read, SETUP_TIMEOUT_SECS).expect("status"),
        ChildSetupStatus::MalformedSetupEvidence(MalformedChildSetupEvidence::InvalidLength {
            len: 3
        },),
    );
}

#[test]
fn child_setup_status_times_out_when_nonblocking_pipe_has_no_status() {
    let (read, _write) = nonblocking_read_pipe();

    let error = read_child_setup_status(&read, 0).expect_err("timeout");

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
}

fn pipe() -> (OwnedFd, OwnedFd) {
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
    unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
}

fn nonblocking_read_pipe() -> (OwnedFd, OwnedFd) {
    let (read, write) = pipe();
    let flags = unsafe { libc::fcntl(read.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(read.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0,
    );
    (read, write)
}

fn write_all(fd: &OwnedFd, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        let written = unsafe { libc::write(fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        assert!(written > 0);
        bytes = &bytes[written as usize..];
    }
}

// PEI-354. The mechanism the defensive clone3 branches rely on: writing "1" to
// `cgroup.kill` through the directory fd the clone itself used, rather than by
// path — so there is no window in which the tree could have been replaced, and
// no path to reconstruct.
//
// The branches themselves are not reachable from a test (they need clone3 to
// return success without setting the pidfd, which is a kernel bug or an
// argument-structure mismatch), which is exactly why the piece that *is*
// testable should be.
#[test]
fn killing_a_cloned_cgroup_writes_through_the_directory_fd() {
    let dir = std::env::temp_dir().join(format!("peinit-cgroup-kill-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("cgroup dir");
    let kill_file = dir.join("cgroup.kill");
    std::fs::write(&kill_file, b"").expect("cgroup.kill");
    let dir_fd = std::fs::File::open(&dir).expect("open cgroup dir");

    super::clone::kill_cloned_cgroup(dir_fd.as_raw_fd());

    assert_eq!(std::fs::read(&kill_file).expect("read back"), b"1");
    std::fs::remove_dir_all(&dir).expect("clean up");
}

/// A cgroup directory with no `cgroup.kill` — a tree already torn down, or a
/// kernel without `cgroup.kill` — must not panic or hang the failing path.
#[test]
fn killing_a_cgroup_without_a_kill_file_is_a_no_op() {
    let dir = std::env::temp_dir().join(format!("peinit-cgroup-nokill-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("cgroup dir");
    let dir_fd = std::fs::File::open(&dir).expect("open cgroup dir");

    super::clone::kill_cloned_cgroup(dir_fd.as_raw_fd());

    std::fs::remove_dir_all(&dir).expect("clean up");
}
