use crate::boundary::{
    BootAttemptCounter, Clock, JobIdentityProvider, ProcessController, ProcessLauncher,
    RealtimeClock, ShutdownFinalizer,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::system::{ControlSecurityDescriptor, SystemAccessChecker};
use crate::jobs::socket::JobsSocketLimits;
use crate::runtime::RuntimeControlLimits;

use super::registration::RuntimeEventRegistrar;

pub struct RuntimeShutdownEventContext<'a, C, P, F, A, R>
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
{
    pub clock: &'a mut C,
    pub controller: &'a mut P,
    pub process_launcher: Option<&'a mut dyn ProcessLauncher>,
    pub finalizer: &'a mut F,
    pub access_checker: &'a mut A,
    pub registrar: &'a mut R,
    pub boot_attempt_counter: &'a mut dyn BootAttemptCounter,
    pub control_security: &'a ControlSecurityDescriptor,
    pub control_limits: RuntimeControlLimits,
    pub job_identity_provider: &'a mut dyn JobIdentityProvider,
    pub jobs_limits: JobsSocketLimits,
}

impl<C, P, F, A, R> RuntimeShutdownEventContext<'_, C, P, F, A, R>
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
{
    /// A shorter-lived view of the same context, so one context can drive
    /// several event turns in sequence.
    pub fn reborrow(&mut self) -> RuntimeShutdownEventContext<'_, C, P, F, A, R> {
        RuntimeShutdownEventContext {
            clock: &mut *self.clock,
            controller: &mut *self.controller,
            process_launcher: self
                .process_launcher
                .as_deref_mut()
                .map(|launcher| launcher as &mut dyn ProcessLauncher),
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
}
