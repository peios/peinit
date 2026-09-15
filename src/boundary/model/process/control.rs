use crate::boundary::BoundaryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessTarget {
    pub service: String,
    pub pid: u32,
    pub pidfd: i32,
    pub cgroup_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessSignal {
    Sigterm,
    Sigkill,
    Sighup,
    Named(String),
    /// A raw signal number, as a submitter's `signal` command names it.
    Number(i32),
}

impl ProcessSignal {
    pub fn name(&self) -> &str {
        match self {
            Self::Sigterm => "SIGTERM",
            Self::Sigkill => "SIGKILL",
            Self::Sighup => "SIGHUP",
            Self::Named(name) => name,
            Self::Number(_) => "signal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupRemoveOutcome {
    Removed,
    Missing,
    Busy,
}

pub trait ProcessController {
    fn pidfd_matches_pid(&mut self, pidfd: i32, pid: u32) -> Result<bool, BoundaryError>;

    /// Close a pidfd the job store has released: its job has finished and left
    /// the store, and nothing else holds the descriptor (PEI-816).
    fn close_pidfd(&mut self, pidfd: i32) -> Result<(), BoundaryError>;

    fn signal_main(
        &mut self,
        target: &ProcessTarget,
        signal: ProcessSignal,
    ) -> Result<(), BoundaryError>;

    fn kill_cgroup(&mut self, cgroup_id: &str) -> Result<(), BoundaryError>;

    /// Whether the cgroup still holds processes.
    ///
    /// A cgroup that does not exist MUST report `false`, not an error: every
    /// caller is asking "may I stop waiting for this to drain?", and a tree
    /// that is gone holds nothing. An error means the cgroup exists and could
    /// not be inspected — "nothing to check" and "cannot check" are different
    /// answers, and only the second is a fault.
    fn cgroup_populated(&mut self, cgroup_id: &str) -> Result<bool, BoundaryError>;

    fn remove_cgroup(&mut self, cgroup_id: &str) -> Result<CgroupRemoveOutcome, BoundaryError>;
}

impl<T: ProcessController + ?Sized> ProcessController for &mut T {
    fn pidfd_matches_pid(&mut self, pidfd: i32, pid: u32) -> Result<bool, BoundaryError> {
        (**self).pidfd_matches_pid(pidfd, pid)
    }

    fn close_pidfd(&mut self, pidfd: i32) -> Result<(), BoundaryError> {
        (**self).close_pidfd(pidfd)
    }

    fn signal_main(
        &mut self,
        target: &ProcessTarget,
        signal: ProcessSignal,
    ) -> Result<(), BoundaryError> {
        (**self).signal_main(target, signal)
    }

    fn kill_cgroup(&mut self, cgroup_id: &str) -> Result<(), BoundaryError> {
        (**self).kill_cgroup(cgroup_id)
    }

    fn cgroup_populated(&mut self, cgroup_id: &str) -> Result<bool, BoundaryError> {
        (**self).cgroup_populated(cgroup_id)
    }

    fn remove_cgroup(&mut self, cgroup_id: &str) -> Result<CgroupRemoveOutcome, BoundaryError> {
        (**self).remove_cgroup(cgroup_id)
    }
}
