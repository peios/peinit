use crate::boundary::ChildReap;
use crate::ids::JobId;

use super::super::dispatch::{
    SupervisedSubmittedTerminalDispatch, SupervisorHealthCheckTerminalDispatch,
    SupervisorPostStartHookTerminalDispatch, SupervisorPreStartHookTerminalDispatch,
    SupervisorReloadCommandTerminalDispatch, SupervisorShutdownTerminalDispatch,
    SupervisorTerminalDispatch,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorChildReapTurn {
    Tracked {
        child: ChildReap,
        job_id: JobId,
        dispatch: SupervisorChildReapDispatch,
    },
    Untracked {
        child: ChildReap,
    },
    /// Reaped before its job recorded a pid; held for replay once it does.
    DeferredUntilSetup {
        child: ChildReap,
    },
    /// Applying the exit raised an internal error, contained to the job's
    /// service: the service is failed and the loop carries on (PEI-1125).
    InternalError {
        child: ChildReap,
        dispatch: Box<super::super::SupervisorInternalErrorDispatch>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorChildReapDispatch {
    Runtime(Box<SupervisorTerminalDispatch>),
    PreStartHook(Box<SupervisorPreStartHookTerminalDispatch>),
    PostStartHook(Box<SupervisorPostStartHookTerminalDispatch>),
    ReloadCommand(Box<SupervisorReloadCommandTerminalDispatch>),
    HealthCheck(Box<SupervisorHealthCheckTerminalDispatch>),
    Shutdown(Box<SupervisorShutdownTerminalDispatch>),
    Submitted(Box<SupervisedSubmittedTerminalDispatch>),
}
