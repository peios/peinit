use crate::boundary::{Clock, ProcessController, RealtimeClock, RegistryClient};
use crate::control::connection::ControlPendingWait;
use crate::control::service_security::ServiceAccessChecker;
use crate::control::service_security::ServiceAccessDenied;
use crate::control::system::{ControlPeer, ControlSecurityDescriptor, SystemAccessChecker};
use crate::control::wire::{ControlBufferConsumeError, ControlFrameRejectReason};
use crate::supervisor::control_command::{
    SupervisorControlCommandBodyError, SupervisorControlCommandDispatch,
};
use crate::supervisor::dispatch::SupervisorSystemShutdownDispatch;
use crate::supervisor::system_shutdown::SupervisorSystemShutdownControlBodyError;

pub struct SupervisorControlFrameContext<'a, 'r, C, P, A>
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
    pub max_request_bytes: usize,
}

pub struct SupervisorShutdownControlFrameContext<'a, C, P, A>
where
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
    A: SystemAccessChecker + ?Sized,
{
    pub peer: &'a ControlPeer,
    pub control_security: &'a ControlSecurityDescriptor,
    pub access_checker: &'a mut A,
    pub controller: &'a mut P,
    pub clock: &'a mut C,
    pub max_request_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorControlConnectionFrameTurn {
    pub frame: SupervisorControlFrameTurn,
    pub pending_write_bytes: usize,
    pub close_after_write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorControlFrameTurn {
    Incomplete {
        buffered_bytes: usize,
    },
    RejectedFrame {
        reason: ControlFrameRejectReason,
        response_line: Vec<u8>,
        close_after_response: bool,
    },
    ShutdownAccepted {
        response_line: Vec<u8>,
        dispatch: Box<SupervisorSystemShutdownDispatch>,
        remaining_bytes: usize,
    },
    ShutdownRejected {
        response_line: Vec<u8>,
        error: SupervisorSystemShutdownControlBodyError,
        remaining_bytes: usize,
    },
    CommandAccepted {
        response_line: Option<Vec<u8>>,
        dispatch: Option<Box<SupervisorControlCommandDispatch>>,
        wait: Option<ControlPendingWait>,
        access_denials: Vec<ServiceAccessDenied>,
        job_access_denials: Vec<crate::submitted::JobAccessDenied>,
        remaining_bytes: usize,
    },
    CommandRejected {
        response_line: Vec<u8>,
        error: SupervisorControlCommandBodyError,
        remaining_bytes: usize,
    },
}

impl SupervisorControlFrameTurn {
    pub fn response_line(&self) -> Option<&[u8]> {
        match self {
            Self::Incomplete { .. } => None,
            Self::RejectedFrame { response_line, .. }
            | Self::ShutdownAccepted { response_line, .. }
            | Self::ShutdownRejected { response_line, .. }
            | Self::CommandRejected { response_line, .. } => Some(response_line),
            Self::CommandAccepted { response_line, .. } => response_line.as_deref(),
        }
    }

    pub fn wait(&self) -> Option<&ControlPendingWait> {
        match self {
            Self::CommandAccepted { wait, .. } => wait.as_ref(),
            Self::Incomplete { .. }
            | Self::RejectedFrame { .. }
            | Self::ShutdownAccepted { .. }
            | Self::ShutdownRejected { .. }
            | Self::CommandRejected { .. } => None,
        }
    }

    pub fn close_after_response(&self) -> bool {
        match self {
            Self::RejectedFrame {
                close_after_response,
                ..
            } => *close_after_response,
            Self::Incomplete { .. }
            | Self::ShutdownAccepted { .. }
            | Self::ShutdownRejected { .. }
            | Self::CommandAccepted { .. }
            | Self::CommandRejected { .. } => false,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SupervisorControlFrameTurnError {
    Buffer(ControlBufferConsumeError),
    ResponseSerialize(String),
}

impl From<serde_json::Error> for SupervisorControlFrameTurnError {
    fn from(error: serde_json::Error) -> Self {
        Self::ResponseSerialize(error.to_string())
    }
}
