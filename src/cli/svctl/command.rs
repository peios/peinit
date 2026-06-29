use std::path::PathBuf;

use crate::shutdown::ShutdownKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub socket_path: PathBuf,
    pub output: OutputMode,
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Human,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Service {
        action: ServiceAction,
        service: String,
        wait: bool,
    },
    Status {
        service: String,
    },
    List,
    OperationStatus {
        operation_id: String,
    },
    ReloadConfig,
    Shutdown {
        kind: ShutdownKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Reload,
    Reset,
}

impl ServiceAction {
    pub fn command_name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Reload => "reload",
            Self::Reset => "reset",
        }
    }

    pub fn default_wait(self) -> bool {
        matches!(self, Self::Start | Self::Stop | Self::Restart)
    }

    pub fn accepts_wait(self) -> bool {
        !matches!(self, Self::Reset)
    }
}
