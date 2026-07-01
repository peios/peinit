mod admin;
mod boot;
mod boot_success;
mod cgroup_cleanup;
mod child_reap;
mod control;
mod control_boundary;
mod control_command;
mod control_connection;
mod dispatch;
mod fd_store_lifecycle;
mod health;
mod launch;
mod lifecycle;
mod lifecycle_deadline_timer;
mod notify;
mod operation_maintenance;
mod phase1_registryd;
mod post_start_hooks;
mod power_button;
mod pre_start_checks;
mod process_setup;
mod ready;
mod relationships;
mod reload_config;
mod restart;
mod shutdown;
mod shutdown_completed;
mod shutdown_deadline_timer;
mod shutdown_driver;
mod shutdown_finalize;
mod shutdown_immediate;
mod shutdown_progress;
mod shutdown_signal;
mod shutdown_starting;
mod shutdown_terminal;
mod shutdown_timeout;
mod shutdown_timeout_actions;
mod shutdown_wave;
mod start_hooks;
mod state;
mod system_shutdown;
mod terminal;
mod timer;
mod watchdog;
mod work;

#[cfg(test)]
mod tests;

pub use child_reap::{SupervisorChildReapDispatch, SupervisorChildReapTurn};
pub use control_boundary::{PendingControlOperation, PendingControlRequirement};
pub use control_command::{
    SupervisorControlCommandBodyContext, SupervisorControlCommandBodyError,
    SupervisorControlCommandBodyResponse, SupervisorControlCommandDispatch,
};
pub use control_connection::{
    SupervisorControlConnectionFrameTurn, SupervisorControlConnectionTableTurn,
    SupervisorControlConnectionTableTurnError, SupervisorControlConnectionTurn,
    SupervisorControlConnectionTurnContext, SupervisorControlConnectionTurnError,
    SupervisorControlFrameTurn, SupervisorControlFrameTurnError, SupervisorControlWaitFlush,
    SupervisorControlWaitFlushError, SupervisorControlWaitFlushTurn,
    SupervisorControlWaitResponseError, SupervisorShutdownControlConnectionTableTurn,
    SupervisorShutdownControlConnectionTableTurnError, SupervisorShutdownControlConnectionTurn,
    SupervisorShutdownControlConnectionTurnContext, SupervisorShutdownControlConnectionTurnError,
};
pub use control_connection::{
    SupervisorControlFrameContext, SupervisorShutdownControlFrameContext,
};
pub use dispatch::{
    SupervisorBootDispatch, SupervisorBootSuccessDispatch, SupervisorControlDispatch,
    SupervisorControlLaunchDispatch, SupervisorControlLaunchResult,
    SupervisorFdStoreRejectionDispatch, SupervisorFilesystemCheckCompletionDispatch,
    SupervisorFilesystemCheckLaunchDispatch, SupervisorFilesystemCheckTimeoutDispatch,
    SupervisorHealthCheckIntervalAction, SupervisorHealthCheckIntervalDispatch,
    SupervisorHealthCheckLaunchCancelledDispatch, SupervisorHealthCheckLaunchDispatch,
    SupervisorHealthCheckLaunchFailureDispatch, SupervisorHealthCheckLaunchResult,
    SupervisorHealthCheckOutcome, SupervisorHealthCheckTerminalDispatch,
    SupervisorHealthCheckTimeoutDispatch, SupervisorImmediateShutdownDispatch,
    SupervisorLaunchDispatch, SupervisorLaunchFailureDispatch, SupervisorLifecycleDispatch,
    SupervisorNotifyDispatch, SupervisorOnFailureLoopSuppressedDispatch,
    SupervisorOnFailureLoopSuppressionReason, SupervisorPendingProcessSetupDispatch,
    SupervisorPostStartHookLaunchDispatch, SupervisorPostStartHookLaunchFailureDispatch,
    SupervisorPostStartHookLaunchResult, SupervisorPostStartHookTerminalDispatch,
    SupervisorPostStartHookTimeoutDispatch, SupervisorPowerButtonAction,
    SupervisorPowerButtonDispatch, SupervisorPreStartHookTerminalDispatch,
    SupervisorPreStartHookTimeoutDispatch, SupervisorProcessSetupDispatch,
    SupervisorReadinessTimeoutDispatch, SupervisorReloadCommandTerminalDispatch,
    SupervisorReloadCommandTimeoutDispatch, SupervisorReloadDetectionDispatch,
    SupervisorRestartBackoffDispatch, SupervisorServiceLaunchDispatch,
    SupervisorShutdownAbandonedDispatch, SupervisorShutdownCgroupKillDispatch,
    SupervisorShutdownDispatch, SupervisorShutdownDriveDispatch,
    SupervisorShutdownFinalizationDispatch, SupervisorShutdownKillDispatch,
    SupervisorShutdownSignalAction, SupervisorShutdownSignalDispatch,
    SupervisorShutdownStopDispatch, SupervisorShutdownTerminalDispatch,
    SupervisorShutdownTimeoutDispatch, SupervisorStartHookLaunchDispatch,
    SupervisorStartHookLaunchFailureDispatch, SupervisorStartHookLaunchResult,
    SupervisorStopEscalationDispatch, SupervisorSystemShutdownDispatch, SupervisorTerminalDispatch,
    SupervisorTimerAction, SupervisorTimerDispatch, SupervisorWatchdogNotifyDispatch,
    SupervisorWatchdogNotifyOutcome, SupervisorWatchdogTimeoutDispatch,
    SupervisorWatchdogTimeoutOutcome,
};
pub use lifecycle_deadline_timer::{
    SupervisorLifecycleDeadline, SupervisorLifecycleDeadlineDispatch,
    SupervisorLifecycleDeadlineKind, SupervisorLifecycleDeadlineTimerTurn,
};
pub use operation_maintenance::SupervisorOperationMaintenanceTurn;
pub use shutdown_deadline_timer::SupervisorShutdownDeadlineTimerTurn;
pub use shutdown_signal::SupervisorPid1SignalFdTurn;
pub use state::{Supervisor, SupervisorError, SupervisorSettings};
pub use system_shutdown::{
    SupervisorSystemShutdownControlBodyError, SupervisorSystemShutdownControlBodyResponse,
    system_shutdown_control_response_line,
};
