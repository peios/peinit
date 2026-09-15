use std::collections::{BTreeMap, BTreeSet};

use crate::ids::{JobId, OperationId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadDetectionDeadline {
    pub operation_id: OperationId,
    pub service: String,
    pub due_at_ns: u64,
    pub phase: ReloadDetectionPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadDetectionPhase {
    DetectionWindow,
    ExtendedWait,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadCommandDeadline {
    pub operation_id: OperationId,
    pub job_id: JobId,
    pub service: String,
    pub cgroup_id: String,
    pub due_at_ns: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct ReloadDeadlineStore {
    detections: BTreeMap<OperationId, ReloadDetectionDeadline>,
    commands: BTreeMap<OperationId, ReloadCommandDeadline>,
    command_ready: BTreeSet<OperationId>,
}

impl ReloadDeadlineStore {
    pub(super) fn record_detection(&mut self, deadline: ReloadDetectionDeadline) {
        self.detections.insert(deadline.operation_id, deadline);
    }

    pub(super) fn due_detections(&self, now_ns: u64) -> Vec<ReloadDetectionDeadline> {
        self.detections
            .values()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .cloned()
            .collect()
    }

    pub(super) fn next_detection(&self) -> Option<ReloadDetectionDeadline> {
        self.detections
            .values()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
            .cloned()
    }

    pub(super) fn remove_detection(
        &mut self,
        operation_id: OperationId,
    ) -> Option<ReloadDetectionDeadline> {
        self.detections.remove(&operation_id)
    }

    pub(super) fn extend_detection(
        &mut self,
        operation_id: OperationId,
        due_at_ns: u64,
    ) -> Option<ReloadDetectionDeadline> {
        let deadline = self.detections.get_mut(&operation_id)?;
        deadline.due_at_ns = due_at_ns;
        Some(deadline.clone())
    }

    pub(super) fn observe_reloading(
        &mut self,
        operation_id: OperationId,
        due_at_ns: u64,
    ) -> Option<ReloadDetectionDeadline> {
        let deadline = self.detections.get_mut(&operation_id)?;
        deadline.due_at_ns = due_at_ns;
        deadline.phase = ReloadDetectionPhase::ExtendedWait;
        Some(deadline.clone())
    }

    pub(super) fn detection(&self, operation_id: OperationId) -> Option<ReloadDetectionDeadline> {
        self.detections.get(&operation_id).cloned()
    }

    pub(super) fn record_command(&mut self, deadline: ReloadCommandDeadline) {
        self.commands.insert(deadline.operation_id, deadline);
    }

    pub(super) fn command(&self, operation_id: OperationId) -> Option<ReloadCommandDeadline> {
        self.commands.get(&operation_id).cloned()
    }

    pub(super) fn due_commands(&self, now_ns: u64) -> Vec<ReloadCommandDeadline> {
        self.commands
            .values()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .cloned()
            .collect()
    }

    pub(super) fn next_command(&self) -> Option<ReloadCommandDeadline> {
        self.commands
            .values()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
            .cloned()
    }

    pub(super) fn remove_command(
        &mut self,
        operation_id: OperationId,
    ) -> Option<ReloadCommandDeadline> {
        self.command_ready.remove(&operation_id);
        self.commands.remove(&operation_id)
    }

    pub(super) fn extend_command(
        &mut self,
        operation_id: OperationId,
        due_at_ns: u64,
    ) -> Option<ReloadCommandDeadline> {
        let deadline = self.commands.get_mut(&operation_id)?;
        deadline.due_at_ns = due_at_ns;
        Some(deadline.clone())
    }

    pub(super) fn has_command(&self, operation_id: OperationId) -> bool {
        self.commands.contains_key(&operation_id)
    }

    pub(super) fn mark_command_ready(&mut self, operation_id: OperationId) {
        self.command_ready.insert(operation_id);
    }

    pub(super) fn take_command_ready(&mut self, operation_id: OperationId) -> bool {
        self.command_ready.remove(&operation_id)
    }

    pub(super) fn cancel_for_service(&mut self, service: &str) -> CancelledReloadDeadlines {
        let detection_ids = self
            .detections
            .iter()
            .filter_map(|(operation_id, deadline)| {
                (deadline.service == service).then_some(*operation_id)
            })
            .collect::<Vec<_>>();
        let command_ids = self
            .commands
            .iter()
            .filter_map(|(operation_id, deadline)| {
                (deadline.service == service).then_some(*operation_id)
            })
            .collect::<Vec<_>>();
        let detections = detection_ids
            .into_iter()
            .filter_map(|operation_id| self.detections.remove(&operation_id))
            .collect();
        let commands = command_ids
            .into_iter()
            .filter_map(|operation_id| self.remove_command(operation_id))
            .collect();
        CancelledReloadDeadlines {
            detections,
            commands,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CancelledReloadDeadlines {
    pub detections: Vec<ReloadDetectionDeadline>,
    pub commands: Vec<ReloadCommandDeadline>,
}
