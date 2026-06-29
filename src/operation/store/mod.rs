mod active;
mod boot;
mod event;
mod replacement;
mod request;
mod terminal;

#[cfg(test)]
mod boot_tests;
#[cfg(test)]
mod tests;

pub use boot::Phase2BootDispatch;
pub use event::{OperationEvent, OperationEventDetail};
pub use request::{OperationRequest, OperationRequestOutcome};
pub use terminal::DEFAULT_TERMINAL_OPERATION_RETENTION_NS;

use std::collections::BTreeMap;

use crate::ids::OperationId;
use crate::operation::conflict::OperationConflictRejection;
use crate::operation::{OperationRecord, OperationState, OperationTransitionError};

pub(in crate::operation::store) use active::InsertPosition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationStoreError {
    DuplicateOperationId {
        id: OperationId,
    },
    UnknownOperation {
        id: OperationId,
    },
    InvalidActiveIndex {
        service: String,
        id: OperationId,
    },
    InvalidEventRecord {
        id: OperationId,
        state: OperationState,
        reason: &'static str,
    },
    ConflictRejected(OperationConflictRejection),
    Transition(OperationTransitionError),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OperationStore {
    records: BTreeMap<OperationId, OperationRecord>,
    active_by_service: BTreeMap<String, Vec<OperationId>>,
}

impl OperationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: OperationId) -> Option<&OperationRecord> {
        self.records.get(&id)
    }
}
