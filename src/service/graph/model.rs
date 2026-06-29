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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceGraphWarning {
    AliveReadinessWithHardDependents {
        service: String,
        dependents: Vec<String>,
    },
}
