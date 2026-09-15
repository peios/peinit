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
    /// Operations the conflict table queued behind another (§8.3), keyed by
    /// the queued operation and naming the one it waits for. A queued
    /// operation is not handed to the control boundary until its
    /// predecessor is terminal; before this existed a queued restart ran
    /// against a service still mid-stop and took PID 1 to recovery
    /// (PEI-824).
    queued_behind: BTreeMap<OperationId, OperationId>,
}

impl OperationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: OperationId) -> Option<&OperationRecord> {
        self.records.get(&id)
    }

    /// Stamp every record that has not yet recorded its target's
    /// `ServiceSecurity` with the descriptor the table holds for it now.
    ///
    /// Requests are made in code that has no view of the service table, so
    /// the record is created without the descriptor and the supervisor
    /// fills it in when it commits the transaction that created it — the
    /// same moment, as far as anything outside the transaction can tell. A
    /// record whose service is already gone keeps `None`; the query path
    /// treats that as the built-in default (PEI-1076).
    pub fn adopt_service_security(&mut self, services: &crate::service::ServiceTable) {
        for record in self.records.values_mut() {
            if record.service_security.is_some() {
                continue;
            }
            if let Some(definition) = services.definition(&record.service) {
                record.service_security = Some(definition.service_security.clone());
            }
        }
    }
}
