mod reload;
mod restart;
mod stop;

pub use reload::{
    CancelledReloadDeadlines, ReloadCommandDeadline, ReloadDetectionDeadline, ReloadDetectionPhase,
};
pub use stop::StopTimeoutDeadline;

use crate::ids::OperationId;

use reload::ReloadDeadlineStore;
use restart::RestartStopLegStore;
use stop::StopTimeoutStore;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ControlExecutionStore {
    stop_timeouts: StopTimeoutStore,
    restart_stop_legs: RestartStopLegStore,
    reload_deadlines: ReloadDeadlineStore,
}

impl ControlExecutionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_stop_timeout(&mut self, deadline: StopTimeoutDeadline) {
        self.stop_timeouts.record(deadline);
    }

    pub fn due_stop_timeouts(&self, now_ns: u64) -> Vec<StopTimeoutDeadline> {
        self.stop_timeouts.due(now_ns)
    }

    pub fn remove_stop_timeout(
        &mut self,
        operation_id: OperationId,
    ) -> Option<StopTimeoutDeadline> {
        self.stop_timeouts.remove(operation_id)
    }

    pub fn stop_timeout(&self, operation_id: OperationId) -> Option<StopTimeoutDeadline> {
        self.stop_timeouts.get(operation_id)
    }

    pub fn stop_timeout_for_service(&self, service: &str) -> Option<StopTimeoutDeadline> {
        self.stop_timeouts.for_service(service)
    }

    pub fn extend_stop_timeout(
        &mut self,
        operation_id: OperationId,
        due_at_ns: u64,
    ) -> Option<StopTimeoutDeadline> {
        self.stop_timeouts.extend(operation_id, due_at_ns)
    }

    pub fn next_stop_timeout(&self) -> Option<StopTimeoutDeadline> {
        self.stop_timeouts.next()
    }

    pub fn record_restart_stop_leg(&mut self, operation_id: OperationId, service: String) {
        self.restart_stop_legs.record(operation_id, service);
    }

    pub fn take_restart_stop_leg(&mut self, operation_id: OperationId) -> Option<String> {
        self.restart_stop_legs.take(operation_id)
    }

    pub fn record_reload_detection_deadline(&mut self, deadline: ReloadDetectionDeadline) {
        self.reload_deadlines.record_detection(deadline);
    }

    pub fn due_reload_detection_deadlines(&self, now_ns: u64) -> Vec<ReloadDetectionDeadline> {
        self.reload_deadlines.due_detections(now_ns)
    }

    pub fn next_reload_detection_deadline(&self) -> Option<ReloadDetectionDeadline> {
        self.reload_deadlines.next_detection()
    }

    pub fn remove_reload_detection_deadline(
        &mut self,
        operation_id: OperationId,
    ) -> Option<ReloadDetectionDeadline> {
        self.reload_deadlines.remove_detection(operation_id)
    }

    pub fn extend_reload_detection_deadline(
        &mut self,
        operation_id: OperationId,
        due_at_ns: u64,
    ) -> Option<ReloadDetectionDeadline> {
        self.reload_deadlines
            .extend_detection(operation_id, due_at_ns)
    }

    pub fn observe_reload_reloading(
        &mut self,
        operation_id: OperationId,
        due_at_ns: u64,
    ) -> Option<ReloadDetectionDeadline> {
        self.reload_deadlines
            .observe_reloading(operation_id, due_at_ns)
    }

    pub fn reload_detection_deadline(
        &self,
        operation_id: OperationId,
    ) -> Option<ReloadDetectionDeadline> {
        self.reload_deadlines.detection(operation_id)
    }

    pub fn record_reload_command_deadline(&mut self, deadline: ReloadCommandDeadline) {
        self.reload_deadlines.record_command(deadline);
    }

    pub fn reload_command_deadline(
        &self,
        operation_id: OperationId,
    ) -> Option<ReloadCommandDeadline> {
        self.reload_deadlines.command(operation_id)
    }

    pub fn due_reload_command_deadlines(&self, now_ns: u64) -> Vec<ReloadCommandDeadline> {
        self.reload_deadlines.due_commands(now_ns)
    }

    pub fn next_reload_command_deadline(&self) -> Option<ReloadCommandDeadline> {
        self.reload_deadlines.next_command()
    }

    pub fn remove_reload_command_deadline(
        &mut self,
        operation_id: OperationId,
    ) -> Option<ReloadCommandDeadline> {
        self.reload_deadlines.remove_command(operation_id)
    }

    pub fn extend_reload_command_deadline(
        &mut self,
        operation_id: OperationId,
        due_at_ns: u64,
    ) -> Option<ReloadCommandDeadline> {
        self.reload_deadlines
            .extend_command(operation_id, due_at_ns)
    }

    pub fn has_reload_command_deadline(&self, operation_id: OperationId) -> bool {
        self.reload_deadlines.has_command(operation_id)
    }

    pub fn mark_reload_command_ready(&mut self, operation_id: OperationId) {
        self.reload_deadlines.mark_command_ready(operation_id);
    }

    pub fn take_reload_command_ready(&mut self, operation_id: OperationId) -> bool {
        self.reload_deadlines.take_command_ready(operation_id)
    }

    pub fn cancel_reload_for_service(&mut self, service: &str) -> CancelledReloadDeadlines {
        self.reload_deadlines.cancel_for_service(service)
    }
}
