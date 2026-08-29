use super::error::BoundaryError;

pub trait Clock {
    fn monotonic_ns(&mut self) -> Result<u64, BoundaryError>;
}

pub trait RealtimeClock {
    fn realtime_ns(&mut self) -> Result<u64, BoundaryError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildReap {
    pub pid: u32,
    pub status: ChildExitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildExitStatus {
    Exited { code: i32 },
    Signaled { signal: i32, core_dumped: bool },
}

pub trait ChildReaper {
    fn reap_children(&mut self) -> Result<Vec<ChildReap>, BoundaryError>;
}

impl<T: Clock + ?Sized> Clock for &mut T {
    fn monotonic_ns(&mut self) -> Result<u64, BoundaryError> {
        (**self).monotonic_ns()
    }
}

impl<T: RealtimeClock + ?Sized> RealtimeClock for &mut T {
    fn realtime_ns(&mut self) -> Result<u64, BoundaryError> {
        (**self).realtime_ns()
    }
}
