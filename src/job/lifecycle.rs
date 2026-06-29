use super::model::{
    JobExit, JobRecord, JobState, JobTransitionAction, JobTransitionError, ProcessHandle,
};

impl JobRecord {
    pub fn start(
        &mut self,
        process: ProcessHandle,
        started_at_ns: u64,
    ) -> Result<(), JobTransitionError> {
        self.ensure_state(JobState::Created, JobTransitionAction::Start)?;
        if started_at_ns < self.created_at_ns {
            return Err(JobTransitionError::StartBeforeCreation {
                id: self.id,
                created_at_ns: self.created_at_ns,
                started_at_ns,
            });
        }
        self.state = JobState::Running;
        self.pid = Some(process.pid);
        self.pidfd = Some(process.pidfd);
        self.started_at_ns = Some(started_at_ns);
        Ok(())
    }

    pub fn complete(&mut self, ended_at_ns: u64, exit_code: i32) -> Result<(), JobTransitionError> {
        self.ensure_state(JobState::Running, JobTransitionAction::Complete)?;
        self.ensure_ended_at(ended_at_ns)?;
        self.state = JobState::Completed;
        self.ended_at_ns = Some(ended_at_ns);
        self.exit_code = Some(exit_code);
        Ok(())
    }

    pub fn fail_before_start(
        &mut self,
        ended_at_ns: u64,
        failure_cause: impl Into<String>,
    ) -> Result<(), JobTransitionError> {
        self.ensure_state(JobState::Created, JobTransitionAction::FailBeforeStart)?;
        self.ensure_ended_at(ended_at_ns)?;
        self.state = JobState::Failed;
        self.ended_at_ns = Some(ended_at_ns);
        self.failure_cause = Some(failure_cause.into());
        Ok(())
    }

    pub fn fail_running(
        &mut self,
        ended_at_ns: u64,
        exit: Option<JobExit>,
        failure_cause: impl Into<String>,
    ) -> Result<(), JobTransitionError> {
        self.ensure_state(JobState::Running, JobTransitionAction::FailRunning)?;
        self.ensure_ended_at(ended_at_ns)?;
        self.state = JobState::Failed;
        self.ended_at_ns = Some(ended_at_ns);
        self.failure_cause = Some(failure_cause.into());
        match exit {
            Some(JobExit::ExitCode(code)) => self.exit_code = Some(code),
            Some(JobExit::Signal(signal)) => self.exit_signal = Some(signal),
            None => {}
        }
        Ok(())
    }

    pub fn abandon(
        &mut self,
        ended_at_ns: u64,
        failure_cause: impl Into<String>,
    ) -> Result<(), JobTransitionError> {
        self.ensure_state(JobState::Running, JobTransitionAction::Abandon)?;
        self.ensure_ended_at(ended_at_ns)?;
        self.state = JobState::Abandoned;
        self.ended_at_ns = Some(ended_at_ns);
        self.failure_cause = Some(failure_cause.into());
        Ok(())
    }

    pub fn duration_ns(&self) -> Option<u64> {
        self.ended_at_ns
            .map(|ended_at_ns| ended_at_ns - self.created_at_ns)
    }

    fn ensure_state(
        &self,
        expected: JobState,
        action: JobTransitionAction,
    ) -> Result<(), JobTransitionError> {
        if self.state == expected {
            Ok(())
        } else {
            Err(JobTransitionError::InvalidTransition {
                id: self.id,
                from: self.state,
                action,
            })
        }
    }

    fn ensure_ended_at(&self, ended_at_ns: u64) -> Result<(), JobTransitionError> {
        if ended_at_ns < self.created_at_ns {
            return Err(JobTransitionError::EndBeforeCreation {
                id: self.id,
                created_at_ns: self.created_at_ns,
                ended_at_ns,
            });
        }
        if let Some(started_at_ns) = self.started_at_ns
            && ended_at_ns < started_at_ns
        {
            return Err(JobTransitionError::EndBeforeStart {
                id: self.id,
                started_at_ns,
                ended_at_ns,
            });
        }
        Ok(())
    }
}
