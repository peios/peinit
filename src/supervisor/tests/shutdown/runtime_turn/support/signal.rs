use std::collections::VecDeque;

use crate::boundary::{LinuxSignalFdRead, LinuxSignalFdReadError};
use crate::runtime::RuntimePid1SignalSource;

#[derive(Debug)]
pub(crate) struct FakeSignalSource {
    reads: VecDeque<LinuxSignalFdRead>,
}

impl FakeSignalSource {
    pub(crate) fn new(reads: impl IntoIterator<Item = LinuxSignalFdRead>) -> Self {
        Self {
            reads: reads.into_iter().collect(),
        }
    }

    pub(crate) fn would_block() -> Self {
        Self::new([])
    }
}

impl RuntimePid1SignalSource for FakeSignalSource {
    fn read_pid1_signal(&mut self) -> Result<LinuxSignalFdRead, LinuxSignalFdReadError> {
        Ok(self
            .reads
            .pop_front()
            .unwrap_or(LinuxSignalFdRead::WouldBlock))
    }
}
