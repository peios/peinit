use std::io;
use std::os::fd::{AsRawFd, IntoRawFd, OwnedFd};

use peios::token::Token;

use crate::boundary::{
    BoundaryError, LaunchedProcess, ProcessCleanupEvidence, ProcessCleanupResource,
    ProcessLaunchError, ProcessLaunchSpec, ProcessSetupStatus,
};

use super::cgroup::{create_job_cgroups, open_cgroup_directory};
use super::command::LaunchCommand;
use super::fd::{create_output_pipe, create_read_nonblocking_pipe};

mod child;
mod model;
mod process_control;

use child::{ChildExecSpec, child_exec};
use model::ChildSetupStatus;
use process_control::{
    CloneProcessResult, clone_process_into_cgroup, open_console, open_dev_null, token_from_handle,
};

pub(super) fn launch_linux_process(
    spec: ProcessLaunchSpec<'_>,
) -> Result<LaunchedProcess, BoundaryError> {
    let ProcessLaunchSpec {
        job,
        token,
        environment,
        inherited_fds,
        setup_timeout_secs: _setup_timeout_secs,
        output_pipe_buffer_bytes,
    } = spec;
    let token = token_from_handle(token)?;
    let mut command = match LaunchCommand::new(job, &environment) {
        Ok(command) => command,
        Err(error) => {
            return Err(parent_setup_with_cleanup(
                error,
                close_parent_token_fd(token),
            ));
        }
    };
    if let Err(error) = create_job_cgroups(job) {
        return Err(parent_setup_with_cleanup(
            error,
            close_parent_token_fd(token),
        ));
    }
    let cgroup = match open_cgroup_directory(&job.cgroup_id) {
        Ok(cgroup) => cgroup,
        Err(error) => {
            return Err(parent_setup_with_cleanup(
                error,
                close_parent_token_fd(token),
            ));
        }
    };
    let dev_null = match open_dev_null() {
        Ok(dev_null) => dev_null,
        Err(error) => {
            return Err(parent_setup_with_cleanup(
                error,
                close_parent_launch_fds(token, cgroup),
            ));
        }
    };

    // Terminal-attached services (a TTYPath in the definition) get a live tty
    // on 0/1/2 instead of the daemon /dev/null + capture pipes below. Opened in
    // the parent so the cloned child inherits the fd; the child dups it onto the
    // standard streams and the parent drops its copy after the clone.
    let console = if let Some(console_path) = job.console_path.as_deref() {
        match open_console(console_path) {
            Ok(console) => Some(console),
            Err(error) => {
                return Err(parent_setup_with_cleanup(
                    error,
                    close_parent_launch_fds(token, cgroup),
                ));
            }
        }
    } else {
        None
    };

    let (exec_error_read, exec_error_write) = match create_read_nonblocking_pipe("exec-error") {
        Ok(pipe) => pipe,
        Err(error) => {
            return Err(parent_setup_with_cleanup(
                error,
                close_parent_launch_fds(token, cgroup),
            ));
        }
    };
    let (stdout_read, stdout_write) = match create_output_pipe("stdout", output_pipe_buffer_bytes) {
        Ok(pipe) => pipe,
        Err(error) => {
            return Err(parent_setup_with_cleanup(
                error,
                close_parent_launch_fds(token, cgroup),
            ));
        }
    };
    let (stderr_read, stderr_write) = match create_output_pipe("stderr", output_pipe_buffer_bytes) {
        Ok(pipe) => pipe,
        Err(error) => {
            return Err(parent_setup_with_cleanup(
                error,
                close_parent_launch_fds(token, cgroup),
            ));
        }
    };
    let child = match clone_process_into_cgroup(cgroup.as_raw_fd()) {
        Ok(CloneProcessResult::Child) => {
            child_exec(
                &token,
                &mut command,
                ChildExecSpec {
                    exec_error_read_fd: exec_error_read.as_raw_fd(),
                    exec_error_write_fd: exec_error_write.as_raw_fd(),
                    dev_null_fd: dev_null.as_raw_fd(),
                    stdout_read_fd: stdout_read.as_raw_fd(),
                    stdout_write_fd: stdout_write.as_raw_fd(),
                    stderr_read_fd: stderr_read.as_raw_fd(),
                    stderr_write_fd: stderr_write.as_raw_fd(),
                    console_fd: console.as_ref().map(|console| console.as_raw_fd()),
                    limit_nofile: job.limit_nofile,
                    limit_core: job.limit_core,
                    oom_score_adj: job.oom_score_adj,
                    inherited_fds: inherited_fds.iter().map(|fd| fd.fd.as_raw_fd()).collect(),
                },
            );
        }
        Ok(CloneProcessResult::Parent(child)) => child,
        Err(error) => {
            let cleanup_evidence = close_parent_launch_fds(token, cgroup);
            return Err(parent_setup_with_cleanup(error, cleanup_evidence));
        }
    };

    let cleanup_evidence = close_parent_launch_fds(token, cgroup);
    drop(dev_null);
    drop(console);
    drop(exec_error_write);
    drop(stdout_write);
    drop(stderr_write);

    Ok(LaunchedProcess {
        pid: child.pid as u32,
        pidfd: child.pidfd.into_raw_fd(),
        stdout_fd: Some(stdout_read.into_raw_fd()),
        stderr_fd: Some(stderr_read.into_raw_fd()),
        setup_status_fd: Some(exec_error_read.into_raw_fd()),
        cleanup_evidence,
    })
}

pub(super) fn read_linux_process_setup_status(
    fd: i32,
) -> Result<ProcessSetupStatus, BoundaryError> {
    match process_control::read_child_setup_status_nonblocking(fd) {
        Ok(ChildSetupStatus::Pending) => Ok(ProcessSetupStatus::Pending),
        Ok(ChildSetupStatus::ExecSucceeded) => Ok(ProcessSetupStatus::ExecSucceeded),
        Ok(ChildSetupStatus::SetupFailed(evidence)) => {
            Ok(ProcessSetupStatus::PreExecFailed(evidence.into()))
        }
        Ok(ChildSetupStatus::MalformedSetupEvidence(evidence)) => {
            Ok(ProcessSetupStatus::MalformedPreExec(evidence.to_string()))
        }
        Err(error) => Err(BoundaryError::Process(format!(
            "read exec status fd {fd} failed: {error}",
        ))),
    }
}

fn parent_setup_with_cleanup(
    error: BoundaryError,
    cleanup_evidence: Vec<ProcessCleanupEvidence>,
) -> BoundaryError {
    match error {
        BoundaryError::Process(message) => BoundaryError::ProcessLaunch(
            ProcessLaunchError::parent_setup_with_cleanup(message, cleanup_evidence),
        ),
        other => other,
    }
}

fn close_parent_token_fd(token: Token) -> Vec<ProcessCleanupEvidence> {
    let mut evidence = Vec::new();
    close_with_evidence(
        token.into_raw_fd(),
        ProcessCleanupResource::Token,
        &mut evidence,
    );
    evidence
}

fn close_parent_launch_fds(token: Token, cgroup: OwnedFd) -> Vec<ProcessCleanupEvidence> {
    let mut evidence = close_parent_token_fd(token);
    close_with_evidence(
        cgroup.into_raw_fd(),
        ProcessCleanupResource::MainCgroup,
        &mut evidence,
    );
    evidence
}

fn close_with_evidence(
    fd: i32,
    resource: ProcessCleanupResource,
    evidence: &mut Vec<ProcessCleanupEvidence>,
) {
    if unsafe { libc::close(fd) } == 0 {
        return;
    }
    evidence.push(ProcessCleanupEvidence {
        fd,
        resource,
        error: io::Error::last_os_error().to_string(),
    });
}

#[cfg(test)]
mod tests {
    use crate::boundary::ProcessCleanupResource;

    use super::close_with_evidence;

    #[test]
    fn close_with_evidence_records_close_failure_without_panicking() {
        let mut evidence = Vec::new();

        close_with_evidence(-1, ProcessCleanupResource::Token, &mut evidence);

        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].fd, -1);
        assert_eq!(evidence[0].resource, ProcessCleanupResource::Token);
        assert!(evidence[0].error.contains("Bad file descriptor"));
    }
}
