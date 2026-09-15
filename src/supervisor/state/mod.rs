mod accessors;
mod deadlines;
mod error;
mod queries;
mod settings;

pub use error::SupervisorError;
pub use settings::SupervisorSettings;

use std::collections::{BTreeMap, VecDeque};

use crate::boundary::ChildExitStatus;
use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::execution::control::ControlExecutionStore;
use crate::execution::graph::GraphExecutionStore;
use crate::execution::launch::{LaunchCreatedJobDispatch, PendingLaunchSetup};
use crate::execution::start::StartExecutionStore;
use crate::fd_store::FdStoreTable;
use crate::ids::{JobId, JobIdAllocator, OperationIdAllocator};
use crate::job::JobStore;
use crate::jobs::socket::JobsSocketLimits;
use crate::logging::RuntimeLogConfig;
use crate::operation::store::OperationStore;
use crate::service::{ServiceEnvironmentVariable, ServiceTable};
use crate::shutdown::{ShutdownRuntime, ShutdownSignalTracker};
use crate::submitted::SubmittedJobStore;

use super::boot_settle::BootSettleTracker;
use super::boot_success::BootSuccessTracker;
use super::cgroup_cleanup::CgroupCleanupStore;
use super::control_boundary::PendingControlOperation;
use super::health::HealthCheckStore;
use super::relationships::RelationshipStore;
use super::watchdog::WatchdogStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supervisor {
    pub(super) settings: SupervisorSettings,
    pub(super) services: ServiceTable,
    pub(super) operations: OperationStore,
    pub(super) jobs: JobStore,
    pub(super) graph: GraphExecutionStore,
    pub(super) control: ControlExecutionStore,
    pub(super) cgroup_cleanup: CgroupCleanupStore,
    pub(super) start: StartExecutionStore,
    pub(super) health: HealthCheckStore,
    pub(super) watchdog: WatchdogStore,
    pub(super) control_security: ControlSecurityDescriptor,
    pub(super) control_limits: ControlSocketLimits,
    pub(super) jobs_limits: JobsSocketLimits,
    pub(super) log_config: RuntimeLogConfig,
    pub(super) global_environment: Vec<ServiceEnvironmentVariable>,
    pub(super) eventd_log_socket_path: Option<String>,
    pub(super) fd_store: FdStoreTable,
    pub(super) relationships: RelationshipStore,
    pub(super) operation_ids: OperationIdAllocator,
    pub(super) job_ids: JobIdAllocator,
    pub(super) pending_launches: VecDeque<JobId>,
    pub(super) pending_start_hook_launches: VecDeque<JobId>,
    pub(super) pending_post_hook_launches: VecDeque<JobId>,
    pub(super) pending_control_launches: VecDeque<JobId>,
    pub(super) pending_health_launches: VecDeque<JobId>,
    pub(super) submitted: SubmittedJobStore,
    pub(super) pending_submitted_launches: VecDeque<JobId>,
    pub(super) pending_process_setups: BTreeMap<i32, PendingLaunchSetup>,
    /// Exits reaped before the job could record its pid, by pid.
    ///
    /// A launched process is only findable by pid once its setup status
    /// has been read and the job started. A short-lived child can be gone
    /// before that, and its exit would otherwise be dropped on the floor —
    /// leaving a job Running against a process that is already reaped, with
    /// no second SIGCHLD ever coming. Held here until the job exists.
    pub(super) reaped_before_setup: BTreeMap<u32, ChildExitStatus>,
    /// Queued launch ids dropped because their job record had gone.
    ///
    /// Counted rather than fatal: see [`crate::supervisor::pending_queue`].
    /// Drained by the work pump so a bookkeeping fault surfaces as a number.
    pub(super) stale_launch_entries: usize,
    pub(super) pending_control_operations: VecDeque<PendingControlOperation>,
    /// Control operations the boundary refused to begin since the work pump
    /// last drained them. Failed rather than fatal: see
    /// [`super::SupervisorControlFailureDispatch`].
    pub(super) control_operation_failures: Vec<super::SupervisorControlFailureDispatch>,
    /// Due restarts peinit could not execute since the deadline timer last
    /// drained them. Failed rather than fatal: see
    /// [`super::SupervisorRestartBackoffFailureDispatch`].
    pub(super) restart_backoff_failures: Vec<super::SupervisorRestartBackoffFailureDispatch>,
    pub(super) retained_service_launches: Vec<LaunchCreatedJobDispatch>,
    pub(super) boot_settle: BootSettleTracker,
    pub(super) boot_success: BootSuccessTracker,
    pub(super) shutdown: Option<ShutdownRuntime>,
    pub(super) shutdown_signals: ShutdownSignalTracker,
    /// A Critical reboot observed by a path that was asked not to finalise
    /// it, for the reconciliation pass to raise with its cause intact.
    pub(super) deferred_critical_reboot: Option<super::critical_budget::DeferredCriticalReboot>,
}

impl Supervisor {
    pub fn new(settings: SupervisorSettings) -> Self {
        Self {
            settings,
            services: ServiceTable::new(),
            operations: OperationStore::new(),
            jobs: JobStore::new(),
            graph: GraphExecutionStore::new(),
            control: ControlExecutionStore::new(),
            cgroup_cleanup: CgroupCleanupStore::new(),
            start: StartExecutionStore::new(),
            health: HealthCheckStore::new(),
            watchdog: WatchdogStore::new(),
            control_security: ControlSecurityDescriptor::Default,
            control_limits: ControlSocketLimits::default(),
            jobs_limits: JobsSocketLimits::default(),
            log_config: RuntimeLogConfig::default(),
            global_environment: Vec::new(),
            eventd_log_socket_path: None,
            fd_store: FdStoreTable::new(),
            relationships: RelationshipStore::new(),
            operation_ids: OperationIdAllocator::new(),
            job_ids: JobIdAllocator::new(),
            pending_launches: VecDeque::new(),
            pending_start_hook_launches: VecDeque::new(),
            pending_post_hook_launches: VecDeque::new(),
            pending_control_launches: VecDeque::new(),
            pending_health_launches: VecDeque::new(),
            submitted: SubmittedJobStore::new(),
            pending_submitted_launches: VecDeque::new(),
            pending_process_setups: BTreeMap::new(),
            reaped_before_setup: BTreeMap::new(),
            stale_launch_entries: 0,
            pending_control_operations: VecDeque::new(),
            control_operation_failures: Vec::new(),
            restart_backoff_failures: Vec::new(),
            retained_service_launches: Vec::new(),
            boot_settle: BootSettleTracker::default(),
            boot_success: BootSuccessTracker::default(),
            shutdown: None,
            shutdown_signals: ShutdownSignalTracker::default(),
            deferred_critical_reboot: None,
        }
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new(SupervisorSettings::default())
    }
}
