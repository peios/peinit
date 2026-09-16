use crate::service::ServiceDependencyKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceGraphValidation {
    pub service_count: usize,
    pub warnings: Vec<ServiceGraphWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceGraphValidationFailure {
    pub findings: Vec<ServiceGraphFinding>,
    /// The warnings the graph would have carried had it validated. A boot
    /// blocks the services its findings name and starts the rest, so the
    /// warnings about the rest are still news (PEI-1124); a reload rejects
    /// the whole transaction and has no use for them.
    pub warnings: Vec<ServiceGraphWarning>,
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

impl ServiceGraphWarning {
    /// The one-line wording of the warning, shared by the console line and
    /// the `graph.validation_warning` event's `message`, so an operator who
    /// saw one can find the other with the same words.
    pub fn message(&self) -> String {
        match self {
            Self::AliveReadinessWithHardDependents { service, .. } => format!(
                "service {service} uses Alive readiness while hard dependents require readiness"
            ),
            Self::UnfilledRole { role, .. } => {
                format!("no service provides {role}, which other services need to start")
            }
        }
    }
}
