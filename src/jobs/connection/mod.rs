//! Per-connection state on the jobs channel: who connected, what they are
//! waiting on, and what is queued to go back to them.

mod accept;
mod record;
mod state;
mod table;

pub use accept::{
    JobsAcceptedConnection, JobsConnectionAcceptError, JobsConnectionAcceptTurn, JobsListener,
    JobsPeer, accept_jobs_connection_at,
};
pub use record::{JobsConnectionFlush, JobsConnectionIo, JobsConnectionRecord};
pub use state::{JobsConnectionState, JobsOutgoing, JobsPendingWait};
pub use table::JobsConnectionTable;
