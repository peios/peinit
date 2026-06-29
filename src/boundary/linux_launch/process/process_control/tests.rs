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
