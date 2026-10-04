use crate::boot::{BootMode, BootModeReason};
use crate::ids::{JobId, OperationId};
use crate::job::JobType;
use crate::operation::{OperationSource, OperationState, OperationType};
use crate::service::runtime::{ServiceHealthStatus, ServiceState, TransitionCause};
use crate::submitted::{JobProgress, JobProgressUnit};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatusView {
    pub service: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub state: ServiceState,
    pub cause: Option<TransitionCause>,
    pub generation: u64,
    pub status_text: Option<String>,
    /// The most recent accepted `PROGRESS=` and `PROGRESS_UNIT=` (PSPU
    /// §4.19), exposed as one `progress` object.
    pub progress: Option<JobProgress>,
    pub progress_unit: Option<JobProgressUnit>,
    pub health: Option<ServiceHealthStatus>,
    pub definition_removed: bool,
    pub current_job: Option<CurrentJobView>,
    pub current_operation: Option<CurrentOperationView>,
    pub warnings: Vec<ServiceStatusWarning>,
    pub lifecycle_warnings: Vec<String>,
    /// Its calendar timers, as the runtime has them armed.
    pub timers: Vec<ServiceTimerView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceListItem {
    pub service: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub state: ServiceState,
    pub cause: Option<TransitionCause>,
    pub health: Option<ServiceHealthStatus>,
    pub definition_removed: bool,
    /// When the soonest of its armed calendar timers fires, CLOCK_REALTIME.
    pub next_timer_ns: Option<u64>,
}

/// One calendar timer trigger of a service: when it fires next, or why it
/// does not fire at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceTimerView {
    pub schedule: String,
    pub arming: ServiceTimerArming,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceTimerArming {
    /// Times are CLOCK_REALTIME nanoseconds.
    Armed {
        /// The schedule's next occurrence.
        scheduled_ns: u64,
        /// When it will fire: the occurrence, delayed by the `TimerJitter`
        /// drawn for it.
        fires_ns: u64,
        /// When it last fired: this uptime, or for a persistent timer not
        /// yet fired this uptime, the `LastTimerRun` read at boot.
        last_fired_ns: Option<u64>,
    },
    /// The schedule did not parse, or has no occurrence in the next ten
    /// years (§9.2). Words, for a person.
    NotArmed { reason: String },
}

impl ServiceTimerView {
    pub fn fires_ns(&self) -> Option<u64> {
        match self.arming {
            ServiceTimerArming::Armed { fires_ns, .. } => Some(fires_ns),
            ServiceTimerArming::NotArmed { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentJobView {
    pub id: JobId,
    pub job_type: JobType,
    pub pid: Option<u32>,
    pub started_at_ns: Option<u64>,
    pub identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentOperationView {
    pub id: OperationId,
    pub operation_type: OperationType,
    pub source: OperationSource,
    pub state: OperationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatusWarning {
    pub path: String,
    pub warning_type: ServiceStatusWarningType,
    pub detected_at_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatusWarningType {
    ServiceTree,
    Health,
    Hooks,
    Helper,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStatusView {
    pub id: OperationId,
    pub operation_type: OperationType,
    pub service: String,
    pub source: OperationSource,
    pub state: OperationState,
    pub created_at_ns: u64,
    pub started_at_ns: Option<u64>,
    pub completed_at_ns: Option<u64>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub merged_into: Option<OperationId>,
}

/// How this boot went, as `boot` reports it (PSPU §4.15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootStatusView {
    /// The mode Phase 2 booted in, after any downgrade.
    pub mode: BootMode,
    pub reason: BootModeReason,
    /// Every finding that forced a downgrade to Safe, in words; empty
    /// unless `reason` is a downgrade.
    pub downgrade: Vec<String>,
    /// The boot attempt counter as the recovery threshold was checked
    /// against it at this boot.
    pub attempts: u32,
    /// The recovery threshold; 0 when the check is disabled.
    pub max_attempts: u32,
    /// The boot has counted as a success, and the counter was reset.
    pub confirmed: bool,
    /// `BootSuccessGrace`: how long the Critical services must hold.
    pub grace_seconds: u32,
    /// Critical services not yet holding a dependent-satisfying state.
    pub waiting_on: Vec<String>,
    /// When the boot will count as a success if nothing changes, on the
    /// monotonic clock: known once every Critical service is holding.
    pub confirms_at_ns: Option<u64>,
    /// Why the counter could not be reset when the grace elapsed.
    pub confirm_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    UnknownService { service: String },
    UnknownOperation { operation_id: OperationId },
    MissingCurrentJobRecord { service: String, job_id: JobId },
}
