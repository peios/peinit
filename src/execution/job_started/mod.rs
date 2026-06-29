mod dispatch;
mod model;

#[cfg(test)]
mod tests;

pub use dispatch::apply_service_main_job_started;
pub use model::{ServiceMainJobStartedDispatch, ServiceMainJobStartedError};
