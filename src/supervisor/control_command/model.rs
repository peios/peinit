use crate::boundary::{Clock, ProcessController, RealtimeClock, RegistryClient};
use crate::control::connection::ControlPendingWait;
use crate::control::reload_config::ReloadConfigOutcome;
use crate::control::service_security::ServiceAccessChecker;
use crate::control::service_security::ServiceAccessDenied;
use crate::control::system::{ControlPeer, ControlSecurityDescriptor, SystemAccessChecker};
use crate::supervisor::{SupervisorLifecycleDispatch, SupervisorSystemShutdownDispatch};

use super::SupervisorControlCommandBodyError;

pub struct SupervisorControlCommandBodyContext<'a, 'r, C, P, A>
where
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    A: SystemAccessChecker + ServiceAccessChecker + crate::submitted::JobAccessChecker + ?Sized,
{
    pub peer: &'a ControlPeer,
    pub control_security: &'a ControlSecurityDescriptor,
    pub access_checker: &'a mut A,
    pub controller: &'a mut P,
    pub clock: &'a mut C,
    pub registry: Option<&'r mut (dyn RegistryClient + 'r)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorControlCommandBodyResponse {
    Accepted {
        response_line: Option<Vec<u8>>,
        dispatch: Option<Box<SupervisorControlCommandDispatch>>,
        wait: Option<ControlPendingWait>,
        access_denials: Vec<ServiceAccessDenied>,
        job_access_denials: Vec<crate::submitted::JobAccessDenied>,
    },
    Rejected {
        response_line: Vec<u8>,
        error: SupervisorControlCommandBodyError,
    },
}

impl SupervisorControlCommandBodyResponse {
    pub(in crate::supervisor) fn accepted_response(response_line: Vec<u8>) -> Self {
        Self::Accepted {
            response_line: Some(response_line),
            dispatch: None,
            wait: None,
            access_denials: Vec::new(),
            job_access_denials: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorControlCommandDispatch {
    Shutdown(Box<SupervisorSystemShutdownDispatch>),
    Lifecycle(Box<SupervisorLifecycleDispatch>),
    ReloadConfig(Box<ReloadConfigOutcome>),
    Job(Box<crate::supervisor::dispatch::SupervisorJobsCommandDispatch>),
}
