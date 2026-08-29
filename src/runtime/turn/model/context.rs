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
