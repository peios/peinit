use crate::execution::satisfaction::{StartSatisfactionRequest, apply_start_satisfaction};
use crate::execution::test_support::{StartedBootGraph, satisfaction_request};

pub(super) const ENDED_AT_NS: u64 = 1_000_004_000;

pub(super) fn satisfy_start(fixture: &mut StartedBootGraph) {
    let request = satisfaction_request("app", fixture.started_operation_id);
    apply_start_satisfaction(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        StartSatisfactionRequest {
            satisfied_at_ns: ENDED_AT_NS - 1,
            ..request
        },
    )
    .expect("satisfy start");
}
