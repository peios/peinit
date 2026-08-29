//! Submitted jobs in the supervisor (PSPU §7): accepting a submission,
//! launching it through the ordinary child path, answering the commands on
//! both sockets, and holding its deadlines and its end.

mod commands;
mod connection;
mod control;
mod deadlines;
mod error;
mod launch;
mod notify;
mod setup;
mod shutdown;
mod stop;
mod submit;
mod terminal;
mod view;
mod wait;

pub use commands::{
    JobsResponseFrame, SupervisorJobsMessageContext, SupervisorJobsMessageResponse,
};
pub use connection::{
    SupervisorJobsConnectionRead, SupervisorJobsConnectionTurn,
    SupervisorJobsConnectionTurnContext, SupervisorJobsConnectionTurnError,
};
pub use error::JobsCommandError;
pub use notify::{JOB_STATUS_EVENT_INTERVAL_NS, SupervisorNotifyOutcome};
pub use wait::{
    SupervisorJobsWaitFlush, SupervisorJobsWaitFlushError, SupervisorJobsWaitFlushTurn,
};

pub(in crate::supervisor) use deadlines::process_submitted_deadline as process_submitted_deadline_in_work;
pub(in crate::supervisor) use launch::apply_started_submitted_launch;
pub(in crate::supervisor) use setup::apply_submitted_setup_failure;
pub(in crate::supervisor) use shutdown::{
    kill_all_live_submitted_jobs, live_submitted_jobs_remain, stop_submitted_jobs_for_shutdown,
};
