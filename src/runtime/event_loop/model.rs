use crate::boundary::{
    BootAttemptCounter, Clock, FilesystemCheckHelperLauncher, LinuxEpoll, LinuxEpollEvent,
    LinuxEpollWaitError, ProcessController, ProcessLauncher, RealtimeClock, ShutdownFinalizer,
    TokenProvider,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::socket::{DEFAULT_CONNECTION_TIMEOUT_SECS, DEFAULT_MAX_REQUEST_SIZE_BYTES};
use crate::control::system::{ControlSecurityDescriptor, SystemAccessChecker};
use crate::runtime::{
    RuntimeEventRegistrar, RuntimeEventRegistrationError, RuntimeEventSource,
    RuntimeEventSourceDecodeError, RuntimeEventdLogFlush, RuntimeShutdownEventContext,
    RuntimeShutdownEventTurn, RuntimeShutdownEventTurnError, RuntimeWorkPumpConfig,
    RuntimeWorkPumpContext, RuntimeWorkPumpError, RuntimeWorkPumpTurn,
};

const EPOLL_WAIT_FOREVER_MS: i32 = -1;
pub const DEFAULT_MAX_CONTROL_READ_BYTES: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeControlLimits {
    pub max_read_bytes: usize,
    pub max_request_bytes: usize,
    pub connection_timeout_secs: u64,
}

impl RuntimeControlLimits {
    pub const fn new(
        max_read_bytes: usize,
        max_request_bytes: usize,
        connection_timeout_secs: u64,
    ) -> Self {
        Self {
            max_read_bytes,
            max_request_bytes,
            connection_timeout_secs,
        }
    }
}

impl Default for RuntimeControlLimits {
    fn default() -> Self {
        Self {
            max_read_bytes: DEFAULT_MAX_CONTROL_READ_BYTES,
            max_request_bytes: DEFAULT_MAX_REQUEST_SIZE_BYTES,
            connection_timeout_secs: DEFAULT_CONNECTION_TIMEOUT_SECS,
        }
    }
}

pub trait RuntimeEventWaiter {
    fn wait_runtime_events(
        &mut self,
        max_events: usize,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeEventWaitError>;

    fn wait_runtime_events_timeout(
        &mut self,
        max_events: usize,
        timeout_ms: i32,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeEventWaitError> {
        let _ = timeout_ms;
        self.wait_runtime_events(max_events)
    }
}

impl RuntimeEventWaiter for LinuxEpoll {
    fn wait_runtime_events(
        &mut self,
        max_events: usize,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeEventWaitError> {
        self.wait_runtime_events_timeout(max_events, EPOLL_WAIT_FOREVER_MS)
    }

    fn wait_runtime_events_timeout(
        &mut self,
        max_events: usize,
        timeout_ms: i32,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeEventWaitError> {
        let events = self
            .wait(max_events, timeout_ms)
            .map_err(RuntimeEventWaitError::Wait)?;
        decode_runtime_epoll_events(events)
    }
}

#[derive(Debug)]
pub enum RuntimeEventWaitError {
    Wait(LinuxEpollWaitError),
    Decode {
        event: LinuxEpollEvent,
        source: RuntimeEventSourceDecodeError,
    },
}

pub struct RuntimeShutdownLoopContext<'a, C, P, F, A, R, T, L, B>
where
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: ShutdownFinalizer,
    A: SystemAccessChecker
        + ServiceAccessChecker
        + crate::submitted::JobAccessChecker
        + crate::submitted::JobDescriptorFactory
        + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
    T: TokenProvider + ?Sized,
    L: ProcessLauncher,
    B: FilesystemCheckHelperLauncher + ?Sized,
{
    pub clock: &'a mut C,
    pub controller: &'a mut P,
    pub finalizer: &'a mut F,
    pub access_checker: &'a mut A,
    pub registrar: &'a mut R,
    pub token_provider: &'a mut T,
    pub process_launcher: &'a mut L,
    pub filesystem_check_launcher: &'a mut B,
    pub boot_attempt_counter: &'a mut dyn BootAttemptCounter,
    pub control_security: &'a ControlSecurityDescriptor,
    pub max_events: usize,
    pub control_limits: RuntimeControlLimits,
    pub work_pump: RuntimeWorkPumpConfig,
    pub job_identity_provider: &'a mut dyn crate::boundary::JobIdentityProvider,
    pub jobs_limits: crate::jobs::socket::JobsSocketLimits,
}

impl<'a, C, P, F, A, R, T, L, B> RuntimeShutdownLoopContext<'a, C, P, F, A, R, T, L, B>
where
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    F: ShutdownFinalizer,
    A: SystemAccessChecker
        + ServiceAccessChecker
        + crate::submitted::JobAccessChecker
        + crate::submitted::JobDescriptorFactory
        + ?Sized,
    R: RuntimeEventRegistrar + ?Sized,
    T: TokenProvider + ?Sized,
    L: ProcessLauncher,
    B: FilesystemCheckHelperLauncher + ?Sized,
{
    pub(super) fn event_context<'b>(&'b mut self) -> RuntimeShutdownEventContext<'b, C, P, F, A, R>
    where
        'a: 'b,
    {
        RuntimeShutdownEventContext {
            clock: &mut *self.clock,
            controller: &mut *self.controller,
            process_launcher: Some(&mut *self.process_launcher),
            finalizer: &mut *self.finalizer,
            access_checker: &mut *self.access_checker,
            registrar: &mut *self.registrar,
            boot_attempt_counter: &mut *self.boot_attempt_counter,
            control_security: self.control_security,
            control_limits: self.control_limits,
            job_identity_provider: &mut *self.job_identity_provider,
            jobs_limits: self.jobs_limits,
        }
    }

    pub(super) fn work_pump_context<'b>(&'b mut self) -> RuntimeWorkPumpContext<'b, C, P, T, L, B>
    where
        'a: 'b,
    {
        RuntimeWorkPumpContext {
            clock: &mut *self.clock,
            controller: &mut *self.controller,
            token_provider: &mut *self.token_provider,
            process_launcher: &mut *self.process_launcher,
            filesystem_check_launcher: &mut *self.filesystem_check_launcher,
            config: self.work_pump.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeShutdownLoopTurn {
    pub pre_work: RuntimeWorkPumpTurn,
    pub sources: Vec<RuntimeEventSource>,
    pub turns: Vec<RuntimeShutdownEventTurn>,
    pub post_work: RuntimeWorkPumpTurn,
    pub eventd_flush: RuntimeEventdLogFlush,
}

#[derive(Debug)]
pub enum RuntimeShutdownLoopError {
    Clock(crate::boundary::BoundaryError),
    Kmes(crate::boundary::BoundaryError),
    Wait(RuntimeEventWaitError),
    Work(RuntimeWorkPumpError),
    LogRegistration(RuntimeEventRegistrationError),
    EventRegistration(RuntimeEventRegistrationError),
    CalendarTimerReconfigure(String),
    OperationMaintenance(crate::supervisor::SupervisorError),
    /// Applying an exit that was reaped before its job carried a pid.
    DeferredChildReap(crate::supervisor::SupervisorError),
    ControlWait(crate::supervisor::SupervisorControlWaitFlushError),
    JobsWait(crate::runtime::RuntimeJobsChannelError),
    Event {
        source: RuntimeEventSource,
        error: RuntimeShutdownEventTurnError,
    },
}

pub(super) fn decode_runtime_epoll_events(
    events: Vec<LinuxEpollEvent>,
) -> Result<Vec<RuntimeEventSource>, RuntimeEventWaitError> {
    events
        .into_iter()
        .map(|event| {
            RuntimeEventSource::from_token(event.token)
                .map_err(|source| RuntimeEventWaitError::Decode { event, source })
        })
        .collect()
}
