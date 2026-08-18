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
