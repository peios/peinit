mod access;
mod child;
mod control;
mod deadline;
mod finalizer;
mod notify;
mod registrar;
mod signal;

pub(super) use access::AllowAccessChecker;
pub(super) use child::FakeChildReaper;
pub(super) use control::{FakeAcceptedConnection, FakeControlListener, control_peer};
pub(super) use deadline::{DeadlineTimerCall, FakeDeadlineTimer};
pub(super) use finalizer::{RuntimeFinalizer, RuntimeFinalizerCall};
pub(super) use notify::FakeNotifySource;
pub(super) use registrar::{FakeRegistrar, RegistrarCall};
pub(super) use signal::FakeSignalSource;

use crate::boundary::{BootAttemptCounter, BoundaryError};
use crate::control::system::ControlSecurityDescriptor;
use crate::runtime::{RuntimeControlLimits, RuntimeShutdownEventContext};
use crate::supervisor::tests::{ScriptedClock, TestProcessController};

static DEFAULT_CONTROL_SECURITY: ControlSecurityDescriptor = ControlSecurityDescriptor::Default;

#[derive(Debug)]
pub(super) struct FakeBootAttemptCounter {
    pub(super) reset_calls: usize,
    pub(super) result: Result<(), BoundaryError>,
}

impl Default for FakeBootAttemptCounter {
    fn default() -> Self {
        Self {
            reset_calls: 0,
            result: Ok(()),
        }
    }
}

impl BootAttemptCounter for FakeBootAttemptCounter {
    fn reset_boot_attempt_counter(&mut self) -> Result<(), BoundaryError> {
        self.reset_calls += 1;
        self.result.clone()
    }
}

pub(super) fn context<'a>(
    clock: &'a mut ScriptedClock,
    controller: &'a mut TestProcessController,
    finalizer: &'a mut RuntimeFinalizer,
    access_checker: &'a mut AllowAccessChecker,
    registrar: &'a mut FakeRegistrar,
    boot_attempt_counter: &'a mut FakeBootAttemptCounter,
) -> RuntimeShutdownEventContext<
    'a,
    ScriptedClock,
    TestProcessController,
    RuntimeFinalizer,
    AllowAccessChecker,
    FakeRegistrar,
> {
    RuntimeShutdownEventContext {
        clock,
        controller,
        process_launcher: None,
        finalizer,
        access_checker,
        registrar,
        boot_attempt_counter,
        control_security: &DEFAULT_CONTROL_SECURITY,
        control_limits: RuntimeControlLimits::new(
            1024,
            crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES,
            crate::control::socket::DEFAULT_CONNECTION_TIMEOUT_SECS,
        ),
    }
}
