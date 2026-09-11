#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessLaunchError {
    ParentSetup {
        message: String,
        cleanup_evidence: Vec<ProcessCleanupEvidence>,
    },
    PreExec {
        error: ProcessPreExecError,
        cleanup_evidence: Vec<ProcessCleanupEvidence>,
    },
    MalformedPreExec {
        message: String,
        cleanup_evidence: Vec<ProcessCleanupEvidence>,
    },
}

impl ProcessLaunchError {
    pub fn parent_setup(message: impl Into<String>) -> Self {
        Self::parent_setup_with_cleanup(message, Vec::new())
    }

    pub fn parent_setup_with_cleanup(
        message: impl Into<String>,
        cleanup_evidence: Vec<ProcessCleanupEvidence>,
    ) -> Self {
        Self::ParentSetup {
            message: message.into(),
            cleanup_evidence,
        }
    }

    pub fn pre_exec(
        error: ProcessPreExecError,
        cleanup_evidence: Vec<ProcessCleanupEvidence>,
    ) -> Self {
        Self::PreExec {
            error,
            cleanup_evidence,
        }
    }

    pub fn malformed_pre_exec(
        message: impl Into<String>,
        cleanup_evidence: Vec<ProcessCleanupEvidence>,
    ) -> Self {
        Self::MalformedPreExec {
            message: message.into(),
            cleanup_evidence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCleanupEvidence {
    pub fd: i32,
    pub resource: ProcessCleanupResource,
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessCleanupResource {
    Token,
    MainCgroup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessPreExecError {
    pub step: ProcessPreExecStep,
    pub errno: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessPreExecStep {
    CloseErrorPipeRead,
    SetStdio,
    ResetSignals,
    InstallToken,
    SetRlimits,
    SetOomScore,
    SetWorkingDirectory,
    SetEnvironment,
    SetNotifySocket,
    InjectStoredFileDescriptors,
    CreateSession,
    AcquireControllingTerminal,
    Exec,
}

impl ProcessPreExecStep {
    pub fn id(self) -> u32 {
        match self {
            Self::CloseErrorPipeRead => 1,
            Self::SetStdio => 2,
            Self::ResetSignals => 3,
            Self::InstallToken => 4,
            Self::SetRlimits => 5,
            Self::SetOomScore => 6,
            Self::SetWorkingDirectory => 7,
            Self::SetEnvironment => 8,
            Self::SetNotifySocket => 9,
            Self::InjectStoredFileDescriptors => 10,
            Self::Exec => 11,
            Self::CreateSession => 12,
            Self::AcquireControllingTerminal => 13,
        }
    }

    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            1 => Some(Self::CloseErrorPipeRead),
            2 => Some(Self::SetStdio),
            3 => Some(Self::ResetSignals),
            4 => Some(Self::InstallToken),
            5 => Some(Self::SetRlimits),
            6 => Some(Self::SetOomScore),
            7 => Some(Self::SetWorkingDirectory),
            8 => Some(Self::SetEnvironment),
            9 => Some(Self::SetNotifySocket),
            10 => Some(Self::InjectStoredFileDescriptors),
            11 => Some(Self::Exec),
            12 => Some(Self::CreateSession),
            13 => Some(Self::AcquireControllingTerminal),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::CloseErrorPipeRead => "close-error-pipe-read",
            Self::SetStdio => "set-stdio",
            Self::ResetSignals => "reset-signals",
            Self::InstallToken => "install-token",
            Self::SetRlimits => "set-rlimits",
            Self::SetOomScore => "set-oom-score",
            Self::SetWorkingDirectory => "set-working-directory",
            Self::SetEnvironment => "set-environment",
            Self::SetNotifySocket => "set-notify-socket",
            Self::InjectStoredFileDescriptors => "inject-stored-file-descriptors",
            Self::CreateSession => "create-session",
            Self::AcquireControllingTerminal => "acquire-controlling-terminal",
            Self::Exec => "exec",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessPreExecStep;

    /// TRM §5.3 — the child setup step identifiers. The wire between the child
    /// and the parent over the setup pipe carries the numeric id, so the whole
    /// table has to be stable in both directions. No guest can read the numbers:
    /// they exist only inside PID 1 and on the pipe between it and its
    /// pre-exec child.
    #[test]
    fn pre_exec_step_ids_match_the_wire_table() {
        let table = [
            (1, ProcessPreExecStep::CloseErrorPipeRead),
            (2, ProcessPreExecStep::SetStdio),
            (3, ProcessPreExecStep::ResetSignals),
            (4, ProcessPreExecStep::InstallToken),
            (5, ProcessPreExecStep::SetRlimits),
            (6, ProcessPreExecStep::SetOomScore),
            (7, ProcessPreExecStep::SetWorkingDirectory),
            (8, ProcessPreExecStep::SetEnvironment),
            (9, ProcessPreExecStep::SetNotifySocket),
            (10, ProcessPreExecStep::InjectStoredFileDescriptors),
            (11, ProcessPreExecStep::Exec),
            (12, ProcessPreExecStep::CreateSession),
            (13, ProcessPreExecStep::AcquireControllingTerminal),
        ];
        for (id, step) in table {
            assert_eq!(step.id(), id, "{step:?} must encode as {id}");
            assert_eq!(
                ProcessPreExecStep::from_id(id),
                Some(step),
                "id {id} must decode back to {step:?}",
            );
        }
        // Nothing outside the table decodes.
        assert_eq!(ProcessPreExecStep::from_id(0), None);
        assert_eq!(ProcessPreExecStep::from_id(14), None);
    }

    /// TRM §5.3/§5.4 — identifier 8 is reserved. It maps to the environment
    /// step, which the child never performs: peinit builds the environment in
    /// the parent and applies it with `execve`, so there is no child step that
    /// could fail and emit id 8. The slot is kept so the numbering of every
    /// other step is fixed regardless.
    #[test]
    fn identifier_eight_is_the_reserved_environment_step() {
        assert_eq!(
            ProcessPreExecStep::from_id(8),
            Some(ProcessPreExecStep::SetEnvironment),
        );
        assert_eq!(ProcessPreExecStep::SetEnvironment.id(), 8);
        assert_eq!(ProcessPreExecStep::SetEnvironment.label(), "set-environment");
    }
}
