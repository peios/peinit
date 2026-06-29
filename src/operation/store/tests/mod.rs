mod request;
mod terminal;

use crate::ids::{OperationId, OperationIdAllocator};
use crate::operation::store::{OperationEventDetail, OperationRequest, event::OperationEvent};
use crate::operation::{OperationSource, OperationType};

const OBSERVED_AT_NS: u64 = 1_717_171_717_123_456_789;

fn ids(count: usize) -> Vec<OperationId> {
    OperationIdAllocator::new()
        .allocate_batch(count, OBSERVED_AT_NS)
        .expect("operation ids")
}

fn request(id: OperationId, operation_type: OperationType, created_at_ns: u64) -> OperationRequest {
    OperationRequest {
        id,
        operation_type,
        service: "svc".to_string(),
        source: OperationSource::Admin,
        caller: None,
        created_at_ns,
    }
}

fn event_details(events: &[OperationEvent]) -> Vec<OperationEventDetail> {
    events.iter().map(|event| event.detail.clone()).collect()
}
