use crate::boundary::{
    BoundaryError, LinuxPowerButtonRead, LinuxPowerButtonReadError, LinuxSignalFdRead,
    LinuxTimerFdRead, RegistryWatchEvent, TimerLastRunWriteOutcome,
};
use crate::control::reload_config::{ReloadConfigError, ReloadConfigOutcome};
use crate::jobs::connection::JobsConnectionAcceptTurn;
use crate::runtime::{RuntimeEventSource, RuntimeJobsConnectionTurn, RuntimeLogPipeTurn};
use crate::supervisor::{
    SupervisorChildReapTurn, SupervisorControlConnectionTableTurn,
    SupervisorFilesystemCheckCompletionDispatch, SupervisorLifecycleDeadlineDispatch,
    SupervisorLifecycleDeadlineTimerTurn, SupervisorPendingProcessSetupDispatch,
    SupervisorPid1SignalFdTurn, SupervisorPowerButtonDispatch, SupervisorProcessSetupDispatch,
    SupervisorShutdownDeadlineTimerTurn, SupervisorShutdownDriveDispatch, SupervisorTimerDispatch,
};

use super::notify::{RuntimeNotifyRead, RuntimeNotifySupervisorTurn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeShutdownEventTurn {
    /// Exits that were reaped before their job existed, replayed once it did.
    ///
    /// Carried as its own turn rather than folded into [`Self::Pid1Signal`]
    /// because there is no signal read behind it: the SIGCHLD was handled turns
    /// ago, and only the application of the exit was outstanding.
    DeferredChildReaps {
        child_reaps: Vec<SupervisorChildReapTurn>,
        ended_at_ns: u64,
    },
    Pid1Signal {
        read: LinuxSignalFdRead,
        supervisor: SupervisorPid1SignalFdTurn,
        child_reaps: Vec<SupervisorChildReapTurn>,
        drive: Option<Box<SupervisorShutdownDriveDispatch>>,
        deadline_timer: Option<SupervisorShutdownDeadlineTimerTurn>,
    },
    ControlListener {
        accept: crate::control::connection::ControlConnectionAcceptTurn,
        registration: Option<RuntimeEventSource>,
    },
    ControlConnection {
        fd: i32,
        supervisor: Box<SupervisorControlConnectionTableTurn>,
        deadline_timer: Option<SupervisorShutdownDeadlineTimerTurn>,
    },
    IdleControlConnectionsClosed {
        fds: Vec<i32>,
    },
    StaleControlConnection {
        fd: i32,
    },
    ShutdownDeadlineTimer {
        read: LinuxTimerFdRead,
        drive: Option<Box<SupervisorShutdownDriveDispatch>>,
        deadline_timer: SupervisorShutdownDeadlineTimerTurn,
    },
    LifecycleDeadlineTimer {
        read: LinuxTimerFdRead,
        drive: Option<Box<SupervisorLifecycleDeadlineDispatch>>,
        deadline_timer: SupervisorLifecycleDeadlineTimerTurn,
    },
    Notify {
        read: RuntimeNotifyRead,
        supervisor: Option<RuntimeNotifySupervisorTurn>,
        deadline_timer: Option<SupervisorShutdownDeadlineTimerTurn>,
    },
    ServiceLogPipe {
        pipe: RuntimeLogPipeTurn,
    },
    CalendarTimer {
        fd: i32,
        turn: RuntimeCalendarTimerTurn,
    },
    FilesystemCheckHelper {
        result_fd: i32,
        turn: RuntimeFilesystemCheckHelperTurn,
    },
    FilesystemCheckHelperExit {
        pidfd: i32,
        turn: RuntimeFilesystemCheckHelperTurn,
    },
    RegistryWatch {
        fd: i32,
        turn: RuntimeRegistryWatchTurn,
    },
    ProcessSetup {
        fd: i32,
        turn: RuntimeProcessSetupTurn,
    },
    PowerButton {
        fd: i32,
        turn: RuntimePowerButtonTurn,
    },
    JobsListener {
        accept: JobsConnectionAcceptTurn,
        registration: Option<RuntimeEventSource>,
    },
    JobsConnection {
        fd: i32,
        turn: RuntimeJobsConnectionTurn,
    },
    IdleJobsConnectionsClosed {
        fds: Vec<i32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCalendarTimerTurn {
    NoRuntimeTable {
        fd: i32,
    },
    Read {
        read: LinuxTimerFdRead,
        supervisor: Option<Box<SupervisorTimerDispatch>>,
        last_run_write: Option<Result<TimerLastRunWriteOutcome, BoundaryError>>,
        next_scheduled_ns: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePowerButtonTurn {
    Ignored {
        read: LinuxPowerButtonRead,
    },
    ReadFailed {
        fd: i32,
        error: LinuxPowerButtonReadError,
        source_disabled: bool,
    },
    Shutdown {
        read: LinuxPowerButtonRead,
        supervisor: Box<SupervisorPowerButtonDispatch>,
        deadline_timer: Option<SupervisorShutdownDeadlineTimerTurn>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeFilesystemCheckHelperTurn {
    WouldBlock {
        result_fd: i32,
    },
    Stale {
        fd: i32,
    },
    Completed {
        completion: Box<SupervisorFilesystemCheckCompletionDispatch>,
    },
    ReadFailedClosed {
        result_fd: i32,
        error: BoundaryError,
        completion: Box<SupervisorFilesystemCheckCompletionDispatch>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeRegistryWatchTurn {
    Unavailable {
        fd: i32,
        reason: String,
    },
    NoEvents {
        fd: i32,
    },
    ReadFailed {
        fd: i32,
        error: BoundaryError,
        source_disabled: bool,
    },
    ReloadConfig {
        events: Vec<RegistryWatchEvent>,
        overflow: bool,
        outcome: Box<Result<ReloadConfigOutcome, ReloadConfigError>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeProcessSetupTurn {
    Pending {
        pending: SupervisorPendingProcessSetupDispatch,
    },
    Stale {
        fd: i32,
    },
    Completed {
        supervisor: Box<SupervisorProcessSetupDispatch>,
        log_registrations: Vec<RuntimeEventSource>,
    },
    /// Reading or applying the setup status raised an internal error,
    /// contained to the launching service (PEI-1125). The descriptor is
    /// unregistered and closed; the job is retired.
    InternalError {
        fd: i32,
        dispatch: Box<crate::supervisor::SupervisorInternalErrorDispatch>,
    },
}
