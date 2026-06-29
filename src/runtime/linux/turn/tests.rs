use crate::boundary::{BoundaryError, KmesEvent, KmesEventSink};
use crate::operation::store::OperationEvent;
use crate::operation::{OperationRecord, OperationSource, OperationState, OperationType};
use crate::runtime::RuntimeWorkPumpTurn;
use crate::supervisor::SupervisorOperationMaintenanceTurn;

use super::emit_runtime_loop_kmes_events;

#[test]
fn runtime_loop_kmes_emission_sends_collected_events_to_sink() {
    let operation = operation_completed("app");
    let maintenance = SupervisorOperationMaintenanceTurn {
        operation_timeouts: vec![operation],
        ..SupervisorOperationMaintenanceTurn::default()
    };
    let mut sink = RecordingKmesSink::default();

    emit_runtime_loop_kmes_events(
        &mut sink,
        &RuntimeWorkPumpTurn::default(),
        &maintenance,
        &[],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
    )
    .expect("emit KMES events");

    assert_eq!(sink.event_types(), vec!["operation.completed"]);
}

#[test]
fn runtime_loop_kmes_emission_maps_sink_failure_to_loop_error() {
    let operation = operation_completed("app");
    let maintenance = SupervisorOperationMaintenanceTurn {
        operation_timeouts: vec![operation],
        ..SupervisorOperationMaintenanceTurn::default()
    };
    let mut sink = RecordingKmesSink::failing(BoundaryError::Kmes("device full".to_string()));

    let error = emit_runtime_loop_kmes_events(
        &mut sink,
        &RuntimeWorkPumpTurn::default(),
        &maintenance,
        &[],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
    )
    .expect_err("sink failure");

    assert!(matches!(
        error,
        crate::runtime::RuntimeShutdownLoopError::Kmes(BoundaryError::Kmes(message))
            if message == "device full"
    ));
    assert!(sink.emitted.is_empty());
}

#[derive(Debug, Default)]
struct RecordingKmesSink {
    emitted: Vec<KmesEvent>,
    error: Option<BoundaryError>,
}

impl RecordingKmesSink {
    fn failing(error: BoundaryError) -> Self {
        Self {
            emitted: Vec::new(),
            error: Some(error),
        }
    }

    fn event_types(&self) -> Vec<&str> {
        self.emitted
            .iter()
            .map(|event| event.event_type.as_str())
            .collect()
    }
}

impl KmesEventSink for RecordingKmesSink {
    fn emit_kmes_event(&mut self, event: &KmesEvent) -> Result<(), BoundaryError> {
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        self.emitted.push(event.clone());
        Ok(())
    }
}

fn operation_completed(service: &str) -> OperationEvent {
    OperationEvent::completed(&OperationRecord {
        id: crate::ids::OperationIdAllocator::new()
            .allocate_batch(1, 1)
            .expect("operation id")[0],
        operation_type: OperationType::Start,
        service: service.to_string(),
        state: OperationState::Completed,
        created_at_ns: 1,
        started_at_ns: Some(2),
        completed_at_ns: Some(3),
        source: OperationSource::Admin,
        caller: None,
        result: Some("timed out".to_string()),
        merged_into: None,
    })
    .expect("completed operation")
}
