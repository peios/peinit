mod cgroup;
mod pidfd;
mod signal;

use super::{BoundaryError, CgroupRemoveOutcome, ProcessController, ProcessSignal, ProcessTarget};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LinuxProcessController;

impl LinuxProcessController {
    pub fn new() -> Self {
        Self
    }
}

impl ProcessController for LinuxProcessController {
    fn pidfd_matches_pid(&mut self, pidfd: i32, pid: u32) -> Result<bool, BoundaryError> {
        pidfd::pidfd_matches_pid(pidfd, pid).map_err(|error| {
            BoundaryError::Process(format!(
                "validate pidfd {pidfd} against pid {pid} failed: {error}"
            ))
        })
    }

    fn close_pidfd(&mut self, pidfd: i32) -> Result<(), BoundaryError> {
        pidfd::close_pidfd(pidfd)
            .map_err(|error| BoundaryError::Process(format!("close pidfd {pidfd} failed: {error}")))
    }

    fn signal_main(
        &mut self,
        target: &ProcessTarget,
        process_signal: ProcessSignal,
    ) -> Result<(), BoundaryError> {
        let raw_signal = signal::signal_number(&process_signal)?;
        pidfd::pidfd_send_signal(target.pidfd, raw_signal).map_err(|error| {
            BoundaryError::Process(format!(
                "pidfd_send_signal(pidfd={}, signal={}) for {} pid {} failed: {error}",
                target.pidfd,
                process_signal.name(),
                target.service,
                target.pid,
            ))
        })
    }

    fn kill_cgroup(&mut self, cgroup_id: &str) -> Result<(), BoundaryError> {
        cgroup::kill_cgroup(cgroup_id)
    }

    fn cgroup_populated(&mut self, cgroup_id: &str) -> Result<bool, BoundaryError> {
        cgroup::cgroup_populated(cgroup_id)
    }

    fn remove_cgroup(&mut self, cgroup_id: &str) -> Result<CgroupRemoveOutcome, BoundaryError> {
        cgroup::remove_cgroup_directory(cgroup_id)
    }
}
