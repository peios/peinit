mod accessors;
mod deadlines;
mod error;
mod queries;
mod settings;

pub use error::SupervisorError;
pub use settings::SupervisorSettings;

use std::collections::{BTreeMap, VecDeque};

use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::execution::control::ControlExecutionStore;
use crate::execution::graph::GraphExecutionStore;
use crate::execution::launch::{LaunchCreatedJobDispatch, PendingLaunchSetup};
use crate::execution::start::StartExecutionStore;
use crate::fd_store::FdStoreTable;
use crate::ids::{JobId, JobIdAllocator, OperationIdAllocator};
use crate::job::JobStore;
use crate::logging::RuntimeLogConfig;
use crate::operation::store::OperationStore;
use crate::service::{ServiceEnvironmentVariable, ServiceTable};
use crate::shutdown::{ShutdownRuntime, ShutdownSignalTracker};

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
    pub(super) pending_process_setups: BTreeMap<i32, PendingLaunchSetup>,
    pub(super) pending_control_operations: VecDeque<PendingControlOperation>,
    pub(super) retained_service_launches: Vec<LaunchCreatedJobDispatch>,
    pub(super) boot_settle: BootSettleTracker,
    pub(super) boot_success: BootSuccessTracker,
    pub(super) shutdown: Option<ShutdownRuntime>,
    pub(super) shutdown_signals: ShutdownSignalTracker,
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
            pending_process_setups: BTreeMap::new(),
            pending_control_operations: VecDeque::new(),
            retained_service_launches: Vec::new(),
            boot_settle: BootSettleTracker::default(),
            boot_success: BootSuccessTracker::default(),
            shutdown: None,
            shutdown_signals: ShutdownSignalTracker::default(),
        }
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new(SupervisorSettings::default())
    }
}
