#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Inactive,
    Starting,
    Active,
    Reloading,
    Stopping,
    Completed,
    Backoff,
    Failed,
    Abandoned,
    Skipped,
}

impl ServiceState {
    pub fn satisfies_dependents(self) -> bool {
        matches!(self, Self::Active | Self::Completed | Self::Skipped)
    }

    pub fn process_presence(self) -> ProcessPresence {
        match self {
            Self::Starting => ProcessPresence::Optional,
            Self::Active | Self::Reloading | Self::Stopping | Self::Abandoned => {
                ProcessPresence::Expected
            }
            Self::Inactive | Self::Completed | Self::Backoff | Self::Failed | Self::Skipped => {
                ProcessPresence::None
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessPresence {
    None,
    Optional,
    Expected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceHealthStatus {
    Unknown,
    Healthy,
    Unhealthy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceHealthSnapshot {
    pub status: ServiceHealthStatus,
    pub consecutive_failures: u32,
    pub last_checked_at_ns: Option<u64>,
}

impl ServiceHealthSnapshot {
    pub fn unknown() -> Self {
        Self {
            status: ServiceHealthStatus::Unknown,
            consecutive_failures: 0,
            last_checked_at_ns: None,
        }
    }

    pub fn record_success(&mut self, checked_at_ns: u64) {
        self.status = ServiceHealthStatus::Healthy;
        self.consecutive_failures = 0;
        self.last_checked_at_ns = Some(checked_at_ns);
    }

    pub fn record_failure(&mut self, checked_at_ns: u64) -> u32 {
        self.status = ServiceHealthStatus::Unhealthy;
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.last_checked_at_ns = Some(checked_at_ns);
        self.consecutive_failures
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeakedCgroupKind {
    ServiceTree,
    Health,
    Hooks,
    /// A pre-start check helper's `<service>/checks` cgroup.
    ///
    /// PSD-007 §4.1: such a helper "is leaked and abandoned exactly as a
    /// service process that survives SIGKILL". It used to be abandoned
    /// without being recorded, so it left no trace at all -- and, worse, did
    /// not bump the cgroup generation, so the next start reused a tree
    /// containing an unkillable process.
    Helper,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeakedCgroup {
    pub path: String,
    pub kind: LeakedCgroupKind,
    pub detected_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStoppingTimeoutEvidence {
    pub started_at_ns: u64,
    pub due_at_ns: u64,
    pub cause: TransitionCause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionCause {
    ExplicitStart,
    DependencyStart,
    RestartPolicy,
    BindsToRecovery,
    ExplicitStop,
    ExplicitReload,
    ExplicitReset,
    ConflictEviction,
    BindsToPropagation,
    Timer,
    ShutdownWave,
    ProcessCrash,
    CleanExit,
    CleanExitRestart,
    ReadinessTimeout,
    WatchdogTimeout,
    HealthCheckFailure,
    PreHookFailure,
    ParentSetupFailure,
    PreExecFailure,
    DependencyFailure,
    RestartBudgetExhausted,
    CycleDetected,
    ValidationError,
    AssertionError,
    ConditionSkipped,
    /// Skipped because another service was holding the terminal this one
    /// names in `TTYPath`.
    ///
    /// Its own cause rather than `ConditionSkipped`: nothing in the
    /// definition was unmet, and an operator asking why there is no login
    /// prompt needs to be told the console is taken and by whom, not that a
    /// condition they never wrote did not hold.
    TtyUnavailable,
    ProcessUnkillable,
    /// peinit could not execute a restart it had scheduled: the relaunch a
    /// backoff deadline began was refused by peinit's own bookkeeping.
    ///
    /// Its own cause because the service did nothing: the process that put it
    /// in Backoff is already recorded, and what ended the restart was peinit.
    /// Failed, so that `start` and `reset` work as they do after any other
    /// failure, rather than a Backoff whose deadline will never fire. Until
    /// PEI-808 such a refusal ended the runtime loop instead.
    InternalError,
}

impl TransitionCause {
    pub fn restart_consultation(self) -> RestartConsultation {
        match self {
            Self::ProcessCrash
            | Self::WatchdogTimeout
            | Self::HealthCheckFailure
            | Self::ReadinessTimeout
            | Self::PreHookFailure
            | Self::PreExecFailure
            | Self::ParentSetupFailure => RestartConsultation::RestartEligible,
            Self::CleanExitRestart => RestartConsultation::AlwaysOnly,
            Self::BindsToRecovery => RestartConsultation::BudgetExempt,
            Self::ExplicitStart
            | Self::DependencyStart
            | Self::RestartPolicy
            | Self::ExplicitStop
            | Self::ExplicitReload
            | Self::ExplicitReset
            | Self::ConflictEviction
            | Self::BindsToPropagation
            | Self::Timer
            | Self::ShutdownWave
            | Self::CleanExit
            | Self::ProcessUnkillable
            | Self::RestartBudgetExhausted
            | Self::CycleDetected
            | Self::ValidationError
            | Self::DependencyFailure
            | Self::AssertionError
            | Self::ConditionSkipped
            | Self::TtyUnavailable
            | Self::InternalError => RestartConsultation::Never,
        }
    }

    pub fn triggers_on_failure(self) -> bool {
        !matches!(
            self,
            Self::ShutdownWave
                | Self::ValidationError
                | Self::CycleDetected
                | Self::DependencyFailure
                | Self::AssertionError
                | Self::ConditionSkipped
                | Self::TtyUnavailable
                | Self::InternalError
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartConsultation {
    RestartEligible,
    AlwaysOnly,
    BudgetExempt,
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRuntimeSnapshot {
    pub service: String,
    pub state: ServiceState,
    pub cause: Option<TransitionCause>,
    pub generation: u64,
    pub cgroup_generation: u64,
    pub dependent_satisfied_since_ns: Option<u64>,
    pub consecutive_restart_failures: u32,
    pub restart_backoff_until_ns: Option<u64>,
    pub status_text: Option<String>,
    /// The readiness level this service last published with `LEVEL=`, for
    /// dependents declaring `Requires = ["<service>:<level>"]`.
    ///
    /// Cleared whenever the service leaves the running state, because a
    /// level is a claim about a process that is currently making it. A
    /// stale one would hold a dependent open on a promise nobody is keeping.
    pub level: Option<String>,
    pub stopping_acknowledged: bool,
    pub stopping_timeout: Option<ServiceStoppingTimeoutEvidence>,
    pub pending_timer: bool,
    pub health: ServiceHealthSnapshot,
    pub leaked_cgroups: Vec<LeakedCgroup>,
}

impl ServiceRuntimeSnapshot {
    pub fn inactive(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            state: ServiceState::Inactive,
            cause: None,
            generation: 0,
            cgroup_generation: 0,
            dependent_satisfied_since_ns: None,
            consecutive_restart_failures: 0,
            restart_backoff_until_ns: None,
            status_text: None,
            level: None,
            stopping_acknowledged: false,
            stopping_timeout: None,
            pending_timer: false,
            health: ServiceHealthSnapshot::unknown(),
            leaked_cgroups: Vec::new(),
        }
    }

    pub fn mark_dependent_satisfied_since(&mut self, satisfied_at_ns: u64) {
        if self.state.satisfies_dependents() {
            self.dependent_satisfied_since_ns = Some(satisfied_at_ns);
        }
    }
}
