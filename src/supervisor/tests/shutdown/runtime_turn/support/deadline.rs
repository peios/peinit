use crate::boundary::{
    BoundaryError, LinuxTimerFdRead, LinuxTimerFdReadError, ShutdownDeadlineTimer,
};
use crate::runtime::{RuntimeLifecycleDeadlineTimer, RuntimeShutdownDeadlineTimer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeadlineTimerCall {
    Read,
    Arm(u64),
    Disarm,
}

#[derive(Debug)]
pub(crate) struct FakeDeadlineTimer {
    read: LinuxTimerFdRead,
    pub(crate) calls: Vec<DeadlineTimerCall>,
}

impl FakeDeadlineTimer {
    pub(crate) fn new(read: LinuxTimerFdRead) -> Self {
        Self {
            read,
            calls: Vec::new(),
        }
    }

    pub(crate) fn would_block() -> Self {
        Self::new(LinuxTimerFdRead::WouldBlock)
    }

    pub(crate) fn expired_once() -> Self {
        Self::new(LinuxTimerFdRead::Expired { expirations: 1 })
    }
}

impl ShutdownDeadlineTimer for FakeDeadlineTimer {
    fn arm_absolute_ns(&mut self, deadline_ns: u64) -> Result<(), BoundaryError> {
        self.calls.push(DeadlineTimerCall::Arm(deadline_ns));
        Ok(())
    }

    fn disarm(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(DeadlineTimerCall::Disarm);
        Ok(())
    }
}

impl RuntimeShutdownDeadlineTimer for FakeDeadlineTimer {
    fn read_shutdown_deadline(&mut self) -> Result<LinuxTimerFdRead, LinuxTimerFdReadError> {
        self.calls.push(DeadlineTimerCall::Read);
        Ok(self.read)
    }
}

impl RuntimeLifecycleDeadlineTimer for FakeDeadlineTimer {
    fn read_lifecycle_deadline(&mut self) -> Result<LinuxTimerFdRead, LinuxTimerFdReadError> {
        self.calls.push(DeadlineTimerCall::Read);
        Ok(self.read)
    }
}
