use crate::boundary::{BoundaryError, LinuxSignalFdReadError, LinuxTimerFdReadError};
use crate::notify::NotifySocketReadError;
use crate::supervisor::{SupervisorControlConnectionTableTurnError, SupervisorError};

use super::registration::RuntimeEventRegistrationError;

#[derive(Debug)]
pub enum RuntimeShutdownEventTurnError {
    SignalRead(LinuxSignalFdReadError),
    ChildReap(BoundaryError),
    Clock(BoundaryError),
    NotifyRead(NotifySocketReadError),
    ControlAccept(crate::control::connection::ControlConnectionAcceptError),
    ControlRegistration(RuntimeEventRegistrationError),
    ControlConnection(SupervisorControlConnectionTableTurnError),
    DeadlineRead(LinuxTimerFdReadError),
    JfsRegistration(RuntimeEventRegistrationError),
    EventRegistration(RuntimeEventRegistrationError),
    Supervisor(SupervisorError),
}
