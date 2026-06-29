use crate::execution::notify::NotifyApplyDispatch;
use crate::execution::start::{
    PostStartHookTerminalDispatch, PostStartHookTimeoutDispatch, PreStartHookTerminalDispatch,
    PreStartHookTimeoutDispatch, ReadinessTimeoutDispatch, StartExecutionDispatch,
};
use crate::fd_store::StoreFdOutcome;

use super::watchdog::SupervisorWatchdogNotifyDispatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPreStartHookTerminalDispatch {
    pub terminal: PreStartHookTerminalDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPostStartHookTerminalDispatch {
    pub terminal: PostStartHookTerminalDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPreStartHookTimeoutDispatch {
    pub timeout: PreStartHookTimeoutDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPostStartHookTimeoutDispatch {
    pub timeout: PostStartHookTimeoutDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorReadinessTimeoutDispatch {
    pub timeout: ReadinessTimeoutDispatch,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorNotifyDispatch {
    pub notify: NotifyApplyDispatch,
    pub fd_store_rejections: Vec<SupervisorFdStoreRejectionDispatch>,
    pub watchdog_notifications: Vec<SupervisorWatchdogNotifyDispatch>,
    pub start_dispatches: Vec<StartExecutionDispatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorFdStoreRejectionDispatch {
    pub service: String,
    pub name: String,
    pub outcome: StoreFdOutcome,
}
