mod dispatch;
mod model;

#[cfg(test)]
mod tests;

pub use dispatch::apply_start_failure;
pub use model::{StartFailureDispatch, StartFailureError, StartFailureRequest};
