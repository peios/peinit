use crate::boundary::{BoundaryError, KmesEvent, KmesEventSink};
use crate::operation::store::OperationEvent;
use crate::operation::{OperationRecord, OperationSource, OperationState, OperationType};
use crate::runtime::RuntimeWorkPumpTurn;
use crate::supervisor::SupervisorOperationMaintenanceTurn;

use super::{RuntimeKmesEmitter, emit_runtime_loop_kmes_events};

#[test]
fn runtime_loop_kmes_emission_sends_collected_events_to_sink() {
    let operation = operation_completed("app");
    let maintenance = SupervisorOperationMaintenanceTurn {
        operation_timeouts: vec![operation],
        ..SupervisorOperationMaintenanceTurn::default()
    };
    let mut sink = RecordingKmesSink::default();
    let mut dropped_events = 0;

    let dropped = emit_runtime_loop_kmes_events(
        RuntimeKmesEmitter {
            sink: &mut sink,
            dropped_events: &mut dropped_events,
        },
        &RuntimeWorkPumpTurn::default(),
        &maintenance,
        &[],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
    )
    .expect("emit KMES events");

    assert_eq!(sink.event_types(), vec!["operation.completed"]);
    assert!(dropped.is_empty());
    assert_eq!(dropped_events, 0);
}

/// PEI-1082, PEI-1125 (the event-emit class): an event the ring refuses is
/// about that event — most likely one too large for `MaxEventSize` — and
/// not about the ring. It is dropped, counted, replaced in the trail by an
/// `event.oversized` naming it and its service, and the loop carries on.
/// Until this it was a fatal loop error, and PID 1 entered recovery over an
/// audit record it could not write.
#[test]
fn runtime_loop_kmes_emission_drops_a_refused_event_and_records_the_gap() {
    let maintenance = SupervisorOperationMaintenanceTurn {
        operation_timeouts: vec![operation_completed("app"), operation_completed("db")],
        ..SupervisorOperationMaintenanceTurn::default()
    };
    let mut sink = RecordingKmesSink::refusing_service("app", "No space left on device");
    let mut dropped_events = 0;

    let dropped = emit_runtime_loop_kmes_events(
        RuntimeKmesEmitter {
            sink: &mut sink,
            dropped_events: &mut dropped_events,
        },
        &RuntimeWorkPumpTurn::default(),
        &maintenance,
        &[],
        &RuntimeWorkPumpTurn::default(),
        &SupervisorOperationMaintenanceTurn::default(),
        &[],
    )
    .expect("a refused event is not a loop error");

    // The refused event is replaced by the gap record; the one after it is
    // still emitted.
    assert_eq!(
        sink.event_types(),
        vec!["event.oversized", "operation.completed"]
    );
    assert_eq!(dropped_events, 1);
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].event_type, "operation.completed");
    assert_eq!(dropped[0].service.as_deref(), Some("app"));
    assert_eq!(dropped[0].dropped_total, 1);
    assert!(dropped[0].error.contains("No space left on device"));
    assert_eq!(
        crate::kmes::kmes_event_subject(&sink.emitted[0].payload)
            .0
            .as_deref(),
        Some("app"),
        "the gap record names the service the dropped event was about",
    );
    assert!(
        dropped[0]
            .console_message()
            .contains("event operation.completed for service app"),
        "{}",
        dropped[0].console_message(),
    );
}

/// The small replacement event fits by construction, so a ring that refuses
/// it too is not refusing an event: it is unusable, and that is still the
/// loop's error.
#[test]
fn runtime_loop_kmes_emission_maps_an_unusable_ring_to_loop_error() {
    let operation = operation_completed("app");
    let maintenance = SupervisorOperationMaintenanceTurn {
        operation_timeouts: vec![operation],
        ..SupervisorOperationMaintenanceTurn::default()
    };
    let mut sink =
        RecordingKmesSink::refusing_everything(BoundaryError::Kmes("device full".to_string()));
    let mut dropped_events = 0;

    let error = emit_runtime_loop_kmes_events(
        RuntimeKmesEmitter {
            sink: &mut sink,
            dropped_events: &mut dropped_events,
        },
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
    /// Refuse every event.
    error: Option<BoundaryError>,
    /// Refuse events naming this service, with this error.
    refuse_service: Option<(String, String)>,
}

impl RecordingKmesSink {
    fn refusing_everything(error: BoundaryError) -> Self {
        Self {
            emitted: Vec::new(),
            error: Some(error),
            refuse_service: None,
        }
    }

    fn refusing_service(service: &str, error: &str) -> Self {
        Self {
            emitted: Vec::new(),
            error: None,
            refuse_service: Some((service.to_string(), error.to_string())),
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
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if let Some((service, error)) = &self.refuse_service
            && event.event_type != "event.oversized"
            && crate::kmes::kmes_event_subject(&event.payload).0.as_deref() == Some(service)
        {
            return Err(BoundaryError::Kmes(error.clone()));
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
        service_security: None,
    })
    .expect("completed operation")
}
