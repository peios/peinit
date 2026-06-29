use std::collections::VecDeque;

use crate::boundary::{BoundaryError, ChildReap, ChildReaper};

#[derive(Debug, Default)]
pub(crate) struct FakeChildReaper {
    results: VecDeque<Result<Vec<ChildReap>, BoundaryError>>,
    pub(crate) calls: usize,
}

impl FakeChildReaper {
    pub(crate) fn new(
        results: impl IntoIterator<Item = Result<Vec<ChildReap>, BoundaryError>>,
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

impl ChildReaper for FakeChildReaper {
    fn reap_children(&mut self) -> Result<Vec<ChildReap>, BoundaryError> {
        self.calls += 1;
        self.results.pop_front().unwrap_or_else(|| Ok(Vec::new()))
    }
}
