mod active;
mod dispatch;
mod ended;
mod model;
mod start;
mod stopping;

#[cfg(test)]
mod tests;

pub use dispatch::apply_service_main_job_terminal;
pub use model::{ServiceMainJobTerminalDispatch, ServiceMainJobTerminalError};
