use crate::service::runtime::ServiceState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownKind {
    Poweroff,
    Reboot,
    Halt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownSignal {
    Sigint,
    Sigterm,
    Sigpwr,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShutdownSignalTracker {
    recent_sigint_ns: Vec<u64>,
}

impl ShutdownSignalTracker {
    pub const FORCED_REBOOT_PRESS_COUNT: usize = 3;
    pub const FORCED_REBOOT_WINDOW_NS: u64 = 5_000_000_000;

    pub fn record_sigint(&mut self, observed_at_ns: u64) -> bool {
        self.recent_sigint_ns.retain(|pressed_at_ns| {
            observed_at_ns.saturating_sub(*pressed_at_ns) <= Self::FORCED_REBOOT_WINDOW_NS
        });
        self.recent_sigint_ns.push(observed_at_ns);
        self.recent_sigint_ns.len() >= Self::FORCED_REBOOT_PRESS_COUNT
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownSettings {
    pub global_timeout_secs: u64,
    pub post_kill_timeout_secs: u64,
}

impl ShutdownSettings {
    pub const DEFAULT_GLOBAL_TIMEOUT_SECS: u64 = 90;
    pub const DEFAULT_POST_KILL_TIMEOUT_SECS: u64 = 5;
}

impl Default for ShutdownSettings {
    fn default() -> Self {
        Self {
            global_timeout_secs: Self::DEFAULT_GLOBAL_TIMEOUT_SECS,
            post_kill_timeout_secs: Self::DEFAULT_POST_KILL_TIMEOUT_SECS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownRuntime {
    pub kind: ShutdownKind,
    pub initiated_at_ns: u64,
    pub global_deadline_ns: u64,
    pub plan: ShutdownPlan,
    pub current_wave: usize,
    pub stop_deadlines: Vec<ShutdownStopDeadline>,
    pub post_kill_deadlines: Vec<ShutdownPostKillDeadline>,
    pub finalization: ShutdownFinalizationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownPlan {
    pub completed_to_clear: Vec<String>,
    pub starting_to_kill: Vec<String>,
    pub stop_waves: Vec<ShutdownStopWave>,
    pub ignored: Vec<ShutdownIgnoredService>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownStopWave {
    pub services: Vec<ShutdownStopParticipant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownStopParticipant {
    pub service: String,
    pub state: ServiceState,
    pub already_stopping: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownIgnoredService {
    pub service: String,
    pub state: ServiceState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownStopDeadline {
    pub service: String,
    pub cgroup_id: String,
    pub started_at_ns: u64,
    pub due_at_ns: u64,
    pub wave: usize,
    pub operation_id: Option<crate::ids::OperationId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownPostKillDeadline {
    pub service: String,
    pub cgroup_id: String,
    pub due_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownFinalizationState {
    WaitingForServices,
    Ready,
    Failed {
        message: String,
        next_retry_at_ns: u64,
    },
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownFinalizationReport {
    pub random_seed: CleanupActionResult,
    pub snapshot_mounts: CleanupActionResult,
    pub mount_results: Vec<MountCleanupResult>,
    pub root_remount: CleanupActionResult,
    pub sync_result: CleanupActionResult,
    pub reboot_result: CleanupActionResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownDeadline {
    pub due_at_ns: u64,
    pub kind: ShutdownDeadlineKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownDeadlineKind {
    GlobalTimeout,
    StopTimeout { service: String },
    PostKillTimeout { service: String },
    FinalActionRetry,
    SubmittedJob { job_id: crate::ids::JobId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountCleanupResult {
    pub mount_point: String,
    pub unmount: CleanupActionResult,
    pub remount_readonly: Option<CleanupActionResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupActionResult {
    Ok,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownPlanError {
    MissingRuntime { service: String },
    MissingDefinition { service: String },
    MissingReadyService { service: String },
    DependencyCycle { services: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownError {
    AlreadyInProgress { kind: ShutdownKind },
    Plan(ShutdownPlanError),
    ServiceTable(crate::service::ServiceTableError),
    OperationStore(crate::operation::store::OperationStoreError),
    JobStore(crate::job::JobStoreError),
    MissingRunningService { service: String },
    MissingJobRecord { job_id: crate::ids::JobId },
    JobNotRunning { job_id: crate::ids::JobId },
    MissingProcessHandle { job_id: crate::ids::JobId },
    InvalidTimeoutExtension { service: String, value: String },
    NoShutdownInProgress,
    ShutdownNotReady,
    FinalActionRetryNotDue { next_retry_at_ns: u64, now_ns: u64 },
    Boundary(crate::boundary::BoundaryError),
}
