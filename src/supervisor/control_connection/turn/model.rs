use crate::boundary::{Clock, ProcessController, RealtimeClock, RegistryClient};
use crate::control::connection::{
    ControlConnectionReadTurn, ControlConnectionWriteTurn, ControlConnectionWriteTurnError,
};
use crate::control::service_security::ServiceAccessChecker;
use crate::control::socket::ControlSocketReadError;
use crate::control::system::{ControlSecurityDescriptor, SystemAccessChecker};

use super::super::{SupervisorControlConnectionFrameTurn, SupervisorControlFrameTurnError};

pub struct SupervisorControlConnectionTurnContext<'a, 'r, C, P, A>
where
    C: Clock + RealtimeClock + ?Sized,
    P: ProcessController + ?Sized,
    A: SystemAccessChecker + ServiceAccessChecker + ?Sized,
{
    pub control_security: &'a ControlSecurityDescriptor,
    pub access_checker: &'a mut A,
    pub controller: &'a mut P,
    pub clock: &'a mut C,
    pub registry: Option<&'r mut (dyn RegistryClient + 'r)>,
    pub max_read_bytes: usize,
    pub max_request_bytes: usize,
    pub observed_at_ns: u64,
}

pub struct SupervisorShutdownControlConnectionTurnContext<'a, C, P, A>
where
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
    A: SystemAccessChecker + ?Sized,
{
    pub control_security: &'a ControlSecurityDescriptor,
    pub access_checker: &'a mut A,
    pub controller: &'a mut P,
    pub clock: &'a mut C,
    pub max_read_bytes: usize,
    pub max_request_bytes: usize,
    pub observed_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorControlConnectionTurn {
    pub read: ControlConnectionReadTurn,
    pub frame: Option<SupervisorControlConnectionFrameTurn>,
    pub write: ControlConnectionWriteTurn,
    pub close_connection: bool,
}

#[derive(Debug)]
pub enum SupervisorControlConnectionTurnError {
    Read(ControlSocketReadError),
    Frame(SupervisorControlFrameTurnError),
    Write(ControlConnectionWriteTurnError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownControlConnectionTurn {
    pub read: ControlConnectionReadTurn,
    pub frame: Option<SupervisorControlConnectionFrameTurn>,
    pub write: ControlConnectionWriteTurn,
    pub close_connection: bool,
}

#[derive(Debug)]
pub enum SupervisorShutdownControlConnectionTurnError {
    Read(ControlSocketReadError),
    Frame(SupervisorControlFrameTurnError),
    Write(ControlConnectionWriteTurnError),
}
