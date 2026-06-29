use std::collections::VecDeque;

use crate::notify::{NotifyDatagram, NotifySocketReadError};
use crate::runtime::RuntimeNotifySource;

#[derive(Debug, Default)]
pub(crate) struct FakeNotifySource {
    results: VecDeque<Result<Option<NotifyDatagram>, NotifySocketReadError>>,
    pub(crate) calls: usize,
}

impl FakeNotifySource {
    pub(crate) fn new(
        results: impl IntoIterator<Item = Result<Option<NotifyDatagram>, NotifySocketReadError>>,
    ) -> Self {
        Self {
            results: results.into_iter().collect(),
            calls: 0,
        }
    }

    pub(crate) fn empty() -> Self {
        Self::default()
    }
}

impl RuntimeNotifySource for FakeNotifySource {
    fn read_notify_datagram(&mut self) -> Result<Option<NotifyDatagram>, NotifySocketReadError> {
        self.calls += 1;
        self.results.pop_front().unwrap_or(Ok(None))
    }
}
