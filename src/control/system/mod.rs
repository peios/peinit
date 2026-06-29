mod access;
mod admission;
mod model;

#[cfg(test)]
mod tests;

pub use access::SystemAccessChecker;
#[cfg(feature = "peios-boundary")]
pub use access::{PeiosSystemAccessChecker, peios_control_peer_from_connected_socket};
pub use admission::admit_system_shutdown_command;
pub use model::{
    ControlPeer, ControlSecurityDescriptor, SystemAccess, SystemAccessCheckError,
    SystemAccessCheckRequest, SystemAccessDecision, SystemAccessDenied,
    SystemShutdownCommandAdmissionError, SystemShutdownCommandOutcome,
    SystemShutdownCommandRequest,
};
