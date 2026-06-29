use crate::ids::OperationId;

use super::model::{
    OperationRecord, OperationState, OperationTransitionAction, OperationTransitionError,
};

impl OperationRecord {
    pub fn start(&mut self, started_at_ns: u64) -> Result<(), OperationTransitionError> {
        self.ensure_state(OperationState::Pending, OperationTransitionAction::Start)?;
        if started_at_ns < self.created_at_ns {
            return Err(OperationTransitionError::StartBeforeCreation {
                id: self.id,
                created_at_ns: self.created_at_ns,
                started_at_ns,
            });
        }
        self.state = OperationState::Running;
        self.started_at_ns = Some(started_at_ns);
        Ok(())
    }

    pub fn complete(
        &mut self,
        completed_at_ns: u64,
        result: impl Into<String>,
    ) -> Result<(), OperationTransitionError> {
        self.finish(
            OperationState::Completed,
            OperationTransitionAction::Complete,
            completed_at_ns,
            result,
        )
    }

    pub fn fail(
        &mut self,
        completed_at_ns: u64,
        failure_reason: impl Into<String>,
    ) -> Result<(), OperationTransitionError> {
        self.finish(
            OperationState::Failed,
            OperationTransitionAction::Fail,
            completed_at_ns,
            failure_reason,
        )
    }

    pub fn merge_into(
        &mut self,
        merged_into: OperationId,
        completed_at_ns: u64,
    ) -> Result<(), OperationTransitionError> {
        self.ensure_state(OperationState::Pending, OperationTransitionAction::Merge)?;
        self.ensure_completed_at(completed_at_ns)?;
        self.state = OperationState::Merged;
        self.completed_at_ns = Some(completed_at_ns);
        self.merged_into = Some(merged_into);
        Ok(())
    }

    pub fn cancel(
        &mut self,
        completed_at_ns: u64,
        reason: impl Into<String>,
    ) -> Result<(), OperationTransitionError> {
        self.ensure_state(OperationState::Pending, OperationTransitionAction::Cancel)?;
        self.ensure_completed_at(completed_at_ns)?;
        self.state = OperationState::Cancelled;
        self.completed_at_ns = Some(completed_at_ns);
        self.result = Some(reason.into());
        Ok(())
    }

    pub fn abort(
        &mut self,
        completed_at_ns: u64,
        reason: impl Into<String>,
    ) -> Result<(), OperationTransitionError> {
        self.ensure_state(OperationState::Running, OperationTransitionAction::Abort)?;
        self.ensure_completed_at(completed_at_ns)?;
        self.state = OperationState::Aborted;
        self.completed_at_ns = Some(completed_at_ns);
        self.result = Some(reason.into());
        Ok(())
    }

    pub fn duration_ns(&self) -> Option<u64> {
        self.completed_at_ns
            .map(|completed_at_ns| completed_at_ns - self.created_at_ns)
    }

    fn finish(
        &mut self,
        state: OperationState,
        action: OperationTransitionAction,
        completed_at_ns: u64,
        result: impl Into<String>,
    ) -> Result<(), OperationTransitionError> {
        if self.state != OperationState::Pending && self.state != OperationState::Running {
            return Err(OperationTransitionError::InvalidTransition {
                id: self.id,
                from: self.state,
                action,
            });
        }
        self.ensure_completed_at(completed_at_ns)?;
        self.state = state;
        self.completed_at_ns = Some(completed_at_ns);
        self.result = Some(result.into());
        Ok(())
    }

    fn ensure_state(
        &self,
        expected: OperationState,
        action: OperationTransitionAction,
    ) -> Result<(), OperationTransitionError> {
        if self.state == expected {
            Ok(())
        } else {
            Err(OperationTransitionError::InvalidTransition {
                id: self.id,
                from: self.state,
                action,
            })
        }
    }

    fn ensure_completed_at(&self, completed_at_ns: u64) -> Result<(), OperationTransitionError> {
        if completed_at_ns < self.created_at_ns {
            Err(OperationTransitionError::CompletionBeforeCreation {
                id: self.id,
                created_at_ns: self.created_at_ns,
                completed_at_ns,
            })
        } else {
            Ok(())
        }
    }
}
