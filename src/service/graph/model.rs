use crate::service::ServiceDependencyKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceGraphValidation {
    pub service_count: usize,
    pub warnings: Vec<ServiceGraphWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceGraphValidationFailure {
    pub findings: Vec<ServiceGraphFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceGraphFinding {
    InvalidServiceName {
        service: String,
    },
    DuplicateService {
        service: String,
    },
    MissingHardDependency {
        service: String,
        target: String,
        kind: ServiceDependencyKind,
    },
    Cycle {
        services: Vec<String>,
    },
    ConflictingBootServices {
        service: String,
        target: String,
    },
    InvalidHealthCheckRestartWindow {
        service: String,
        retries: u32,
        interval_secs: u64,
        restart_window_secs: u64,
    },
    InvalidTimerSchedule {
        service: String,
        schedule: String,
        message: String,
    },
    /// A `HealthCheck` on a service that will never run one — health checks
    /// are scheduled for `ServiceType::Simple` alone.
    ///
    /// Said directly rather than via the flap constraint's timing arithmetic,
    /// which is what used to reject these definitions: the operator adjusted
    /// `RestartWindow`, the definition validated, and the check still did
    /// nothing (PEI-367).
    UnschedulableHealthCheck {
        service: String,
        service_type: crate::service::ServiceType,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceGraphWarning {
    AliveReadinessWithHardDependents {
        service: String,
        dependents: Vec<String>,
    },
    /// Services need the authority to start and no service fills the role.
    ///
    /// A warning rather than a finding, deliberately. The image is broken —
    /// none of these services can obtain a token — but failing validation
    /// would reject a whole *reload* transaction, so an operator could not
    /// even reload a definition that adds the missing authority. The
    /// individual launches still fail, loudly, where the fault actually is.
    UnfilledRole { role: String, services: Vec<String> },
}
