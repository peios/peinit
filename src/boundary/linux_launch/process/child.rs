use std::os::fd::AsRawFd;

use peios::token::Token;

use crate::boundary::ProcessPreExecStep;

use super::super::command::LaunchCommand;
use super::super::fd::close_fd;
use super::model::ChildSetupEvidence;

use self::resources::{change_working_directory, set_oom_score_adj, set_rlimits};
use self::session::{acquire_controlling_terminal, become_session_leader};
use self::signals::reset_signal_environment;
use self::stdio::{set_console_streams, set_standard_streams};
use self::sys::{clear_cloexec, close_fd_checked, dup2_checked, errno};

mod resources;
mod session;
mod signals;
mod stdio;
mod sys;

const CHILD_SETUP_EXIT_CODE: libc::c_int = 126;
const CHILD_EXEC_EXIT_CODE: libc::c_int = 127;
const SD_LISTEN_FDS_START: libc::c_int = 3;

pub(super) struct ChildExecSpec {
    pub exec_error_read_fd: i32,
    pub exec_error_write_fd: i32,
    pub dev_null_fd: i32,
    pub stdout_read_fd: i32,
    pub stdout_write_fd: i32,
    pub stderr_read_fd: i32,
    pub stderr_write_fd: i32,
    /// When `Some`, attach this fd (an open `/dev/console`) to stdin/stdout/
    /// stderr instead of the daemon `/dev/null` + capture pipes above.
    pub console_fd: Option<i32>,
    pub limit_nofile: Option<u64>,
    pub limit_core: Option<u64>,
    pub oom_score_adj: i32,
    pub inherited_fds: Vec<i32>,
}

// Runs after clone in the child. Keep this path to prebuilt pointers, raw fd
// integers, and direct syscalls; parent-side launch code must do allocation and
// formatting before reaching this point.
pub(super) fn child_exec(token: &Token, command: &LaunchCommand, spec: ChildExecSpec) -> ! {
    let ChildExecSpec {
        exec_error_read_fd,
        exec_error_write_fd,
        dev_null_fd,
        stdout_read_fd,
        stdout_write_fd,
        stderr_read_fd,
        stderr_write_fd,
        console_fd,
        limit_nofile,
        limit_core,
        oom_score_adj,
        inherited_fds,
    } = spec;

    if let Err(errno) = close_fd_checked(exec_error_read_fd) {
        fail_child_setup(
            exec_error_write_fd,
            ProcessPreExecStep::CloseErrorPipeRead,
            errno,
        );
    }

    // A terminal-attached service owns its terminal: new session first, so the
    // TIOCSCTTY below has a session leader to attach to. Ordered before the
    // dup because setsid() drops any controlling terminal the child inherited,
    // which would otherwise undo the attach.
    if console_fd.is_some()
        && let Err(errno) = become_session_leader()
    {
        fail_child_setup(
            exec_error_write_fd,
            ProcessPreExecStep::CreateSession,
            errno,
        );
    }

    let stdio_result = match console_fd {
        Some(console_fd) => set_console_streams(
            console_fd,
            dev_null_fd,
            stdout_read_fd,
            stdout_write_fd,
            stderr_read_fd,
            stderr_write_fd,
        ),
        None => set_standard_streams(
            dev_null_fd,
            stdout_read_fd,
            stdout_write_fd,
            stderr_read_fd,
            stderr_write_fd,
        ),
    };
    if let Err(errno) = stdio_result {
        fail_child_setup(exec_error_write_fd, ProcessPreExecStep::SetStdio, errno);
    }

    // Now that the terminal is on fd 0, claim it. Deliberately fatal rather
    // than a warning: a shell without a controlling terminal starts and looks
    // fine but has no job control, which is a far more confusing failure to
    // meet later than a refused start naming this step.
    if console_fd.is_some()
        && let Err(errno) = acquire_controlling_terminal()
    {
        fail_child_setup(
            exec_error_write_fd,
            ProcessPreExecStep::AcquireControllingTerminal,
            errno,
        );
    }

    if let Err(errno) = reset_signal_environment() {
        fail_child_setup(exec_error_write_fd, ProcessPreExecStep::ResetSignals, errno);
    }

    if let Err(error) = token.install() {
        fail_child_setup(
            exec_error_write_fd,
            ProcessPreExecStep::InstallToken,
            error.raw_os_error().unwrap_or(libc::EIO),
        );
    }
    close_fd(token.as_raw_fd());

    if let Err(errno) = set_rlimits(limit_nofile, limit_core) {
        fail_child_setup(exec_error_write_fd, ProcessPreExecStep::SetRlimits, errno);
    }

    if let Err(errno) = set_oom_score_adj(oom_score_adj) {
        fail_child_setup(exec_error_write_fd, ProcessPreExecStep::SetOomScore, errno);
    }

    if let Err(errno) = change_working_directory(command.working_directory_ptr()) {
        fail_child_setup(
            exec_error_write_fd,
            ProcessPreExecStep::SetWorkingDirectory,
            errno,
        );
    }

    if !command.environment_contains_notify_socket() {
        fail_child_setup(
            exec_error_write_fd,
            ProcessPreExecStep::SetNotifySocket,
            libc::EINVAL,
        );
    }

    if let Err(errno) = inject_stored_file_descriptors(&inherited_fds) {
        fail_child_setup(
            exec_error_write_fd,
            ProcessPreExecStep::InjectStoredFileDescriptors,
            errno,
        );
    }

    unsafe {
        libc::execve(
            command.program_ptr(),
            command.argv_ptrs(),
            command.env_ptrs(),
        );
    }
    write_child_error_payload(
        exec_error_write_fd,
        ChildSetupEvidence {
            step: ProcessPreExecStep::Exec,
            errno: errno(),
        },
    );
    unsafe { libc::_exit(CHILD_EXEC_EXIT_CODE) }
}

fn inject_stored_file_descriptors(fds: &[i32]) -> Result<(), i32> {
    inject_stored_file_descriptors_at(fds, SD_LISTEN_FDS_START)
}

fn inject_stored_file_descriptors_at(fds: &[i32], start_fd: libc::c_int) -> Result<(), i32> {
    for (index, fd) in fds.iter().copied().enumerate() {
        let offset = libc::c_int::try_from(index).map_err(|_| libc::EINVAL)?;
        let target = start_fd.checked_add(offset).ok_or(libc::EINVAL)?;
        if fd == target {
            clear_cloexec(target)?;
        } else {
            dup2_checked(fd, target)?;
            clear_cloexec(target)?;
            close_fd_checked(fd)?;
        }
    }
    Ok(())
}

fn fail_child_setup(error_fd: i32, step: ProcessPreExecStep, errno: i32) -> ! {
    write_child_error_payload(error_fd, ChildSetupEvidence { step, errno });
    unsafe { libc::_exit(CHILD_SETUP_EXIT_CODE) }
}

fn write_child_error_payload(fd: i32, evidence: ChildSetupEvidence) {
    let bytes = evidence.encode();
    let _ = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
}

#[cfg(test)]
mod tests {
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};

    use super::inject_stored_file_descriptors_at;

    #[test]
    fn stored_fd_injection_closes_temporary_source_and_keeps_target_inheritable() {
        let (read, _write) = pipe();
        let source = duplicate_min(read.as_raw_fd(), 200);
        let source_fd = source.into_raw_fd();
        let target = duplicate_min(read.as_raw_fd(), source_fd + 1);
        let target_fd = target.into_raw_fd();
        unsafe {
            libc::close(target_fd);
        }

        inject_stored_file_descriptors_at(&[source_fd], target_fd).expect("inject fd");

        assert_eq!(fcntl_getfd(source_fd), -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EBADF)
        );
        let target_flags = fcntl_getfd(target_fd);
        assert!(target_flags >= 0);
        assert_eq!(target_flags & libc::FD_CLOEXEC, 0);
        unsafe {
            libc::close(target_fd);
        }
    }

    fn pipe() -> (OwnedFd, OwnedFd) {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
    }

    fn duplicate_min(fd: i32, min_fd: i32) -> OwnedFd {
        let duplicated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, min_fd) };
        assert!(duplicated >= 0);
        unsafe { OwnedFd::from_raw_fd(duplicated) }
    }

    fn fcntl_getfd(fd: i32) -> i32 {
        unsafe { libc::fcntl(fd, libc::F_GETFD) }
    }
}
