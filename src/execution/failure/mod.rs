mod dispatch;
mod model;

#[cfg(test)]
mod tests;

pub use dispatch::{DependentFailures, apply_start_failure, fail_dependents_after_graph_events};
pub use model::{StartFailureDispatch, StartFailureError, StartFailureRequest};
