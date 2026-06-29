mod dispatch;
mod model;

#[cfg(test)]
mod tests;

pub use dispatch::apply_start_satisfaction;
pub use model::{StartSatisfactionDispatch, StartSatisfactionError, StartSatisfactionRequest};
