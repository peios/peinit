mod definition;
mod dependency;
pub mod graph;
mod restart;
pub(crate) mod role;
pub mod table;

pub mod runtime;

pub use definition::{
    ErrorControl, NotifyAccess, Readiness, RestartPolicy, ServiceCheck, ServiceCheckKind,
    ServiceDefinition, ServiceEnvironmentVariable, ServiceSecurityDescriptor, ServiceTrigger,
    ServiceType, is_valid_service_name,
};
pub use dependency::{
    ServiceDependency, ServiceDependencyKind, ServiceDependencyOrderCycle,
    all_declared_dependencies, dependency_start_order, existing_start_order_dependency_targets,
    hard_dependencies, split_target, start_order_dependency_targets,
};
pub use graph::{
    ServiceGraphFinding, ServiceGraphValidation, ServiceGraphValidationFailure,
    ServiceGraphWarning, validate_service_graph,
};
pub use restart::{
    RestartEvaluation, RestartEvaluationAction, evaluate_restart_after_failure,
    is_success_exit_code,
};
pub use role::{AUTHN_ROLE, synthesise_role_dependencies};
pub use table::{
    RestartBackoffDeadline, RestartWindowResetDeadline, ServiceActivationSnapshot, ServiceEntry,
    ServiceReloadSummary, ServiceTable, ServiceTableError, ServiceTableTransition,
};
