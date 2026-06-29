#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorOnFailureLoopSuppressionReason {
    Cycle,
    MaxDepth { max_depth: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorOnFailureLoopSuppressedDispatch {
    pub failed_service: String,
    pub attempted_handler: String,
    pub chain: Vec<String>,
    pub reason: SupervisorOnFailureLoopSuppressionReason,
}
