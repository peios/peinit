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
}

impl ProcessSignal {
    pub fn name(&self) -> &str {
        match self {
            Self::Sigterm => "SIGTERM",
            Self::Sigkill => "SIGKILL",
            Self::Sighup => "SIGHUP",
            Self::Named(name) => name,
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
