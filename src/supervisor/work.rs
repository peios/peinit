//! Transactional supervisor work state.
//!
//! Supervisor methods that touch several stores use this snapshot to preserve
//! atomicity: clone the current state, perform the whole transition, then commit
//! only after every store update and boundary-independent decision succeeds.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::execution::control::ControlExecutionStore;
use crate::execution::graph::{GraphContextId, GraphExecutionEvent, GraphExecutionStore};
use crate::execution::launch::{LaunchCreatedJobDispatch, PendingLaunchSetup};
use crate::execution::start::{StartExecutionJobKind, StartExecutionStore};
use crate::fd_store::FdStoreTable;
use crate::ids::{JobId, JobIdAllocator, OperationIdAllocator};
use crate::job::JobStore;
use crate::jobs::socket::JobsSocketLimits;
use crate::logging::RuntimeLogConfig;
use crate::operation::store::OperationStore;
use crate::service::{ServiceEnvironmentVariable, ServiceTable};
use crate::shutdown::{ShutdownError, ShutdownRuntime, ShutdownSignalTracker};
use crate::submitted::SubmittedJobStore;

use super::boot_settle::BootSettleTracker;
use super::boot_success::BootSuccessTracker;
use super::cgroup_cleanup::CgroupCleanupStore;
use super::control_boundary::PendingControlOperation;
use super::health::HealthCheckStore;
use super::relationships::RelationshipStore;
use super::state::Supervisor;
use super::watchdog::WatchdogStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SupervisorWork {
    pub services: ServiceTable,
    pub operations: OperationStore,
    pub jobs: JobStore,
    pub graph: GraphExecutionStore,
    pub control: ControlExecutionStore,
    pub cgroup_cleanup: CgroupCleanupStore,
    pub start: StartExecutionStore,
    pub health: HealthCheckStore,
    pub watchdog: WatchdogStore,
    pub control_security: ControlSecurityDescriptor,
    pub control_limits: ControlSocketLimits,
    pub jobs_limits: JobsSocketLimits,
    pub log_config: RuntimeLogConfig,
    pub fd_store: FdStoreTable,
    pub relationships: RelationshipStore,
    pub global_environment: Vec<ServiceEnvironmentVariable>,
    pub eventd_log_socket_path: Option<String>,
    pub operation_ids: OperationIdAllocator,
    pub job_ids: JobIdAllocator,
    pub pending_launches: VecDeque<JobId>,
    pub pending_start_hook_launches: VecDeque<JobId>,
    pub pending_post_hook_launches: VecDeque<JobId>,
    pub pending_control_launches: VecDeque<JobId>,
    pub pending_health_launches: VecDeque<JobId>,
    pub submitted: SubmittedJobStore,
    pub pending_submitted_launches: VecDeque<JobId>,
    pub pending_process_setups: BTreeMap<i32, PendingLaunchSetup>,
    pub pending_control_operations: VecDeque<PendingControlOperation>,
    pub retained_service_launches: Vec<LaunchCreatedJobDispatch>,
    pub boot_settle: BootSettleTracker,
    pub boot_success: BootSuccessTracker,
    pub shutdown: Option<ShutdownRuntime>,
    pub shutdown_signals: ShutdownSignalTracker,
}

impl SupervisorWork {
    pub fn from_supervisor(supervisor: &Supervisor) -> Self {
        Self {
            services: supervisor.services.clone(),
            operations: supervisor.operations.clone(),
            jobs: supervisor.jobs.clone(),
            graph: supervisor.graph.clone(),
            control: supervisor.control.clone(),
            cgroup_cleanup: supervisor.cgroup_cleanup.clone(),
            start: supervisor.start.clone(),
            health: supervisor.health.clone(),
            watchdog: supervisor.watchdog.clone(),
            control_security: supervisor.control_security.clone(),
            control_limits: supervisor.control_limits,
            jobs_limits: supervisor.jobs_limits,
            log_config: supervisor.log_config.clone(),
            fd_store: supervisor.fd_store.clone(),
            relationships: supervisor.relationships.clone(),
            global_environment: supervisor.global_environment.clone(),
            eventd_log_socket_path: supervisor.eventd_log_socket_path.clone(),
            operation_ids: supervisor.operation_ids.clone(),
            job_ids: supervisor.job_ids.clone(),
            pending_launches: supervisor.pending_launches.clone(),
            pending_start_hook_launches: supervisor.pending_start_hook_launches.clone(),
            pending_post_hook_launches: supervisor.pending_post_hook_launches.clone(),
            pending_control_launches: supervisor.pending_control_launches.clone(),
            pending_health_launches: supervisor.pending_health_launches.clone(),
            submitted: supervisor.submitted.clone(),
            pending_submitted_launches: supervisor.pending_submitted_launches.clone(),
            pending_process_setups: supervisor.pending_process_setups.clone(),
            pending_control_operations: supervisor.pending_control_operations.clone(),
            retained_service_launches: supervisor.retained_service_launches.clone(),
            boot_settle: supervisor.boot_settle.clone(),
            boot_success: supervisor.boot_success.clone(),
            shutdown: supervisor.shutdown.clone(),
            shutdown_signals: supervisor.shutdown_signals.clone(),
        }
    }

    pub fn commit(self, supervisor: &mut Supervisor) {
        let mut fd_store = self.fd_store;
        fd_store.retain_services(&self.services.service_names());
        supervisor.services = self.services;
        supervisor.operations = self.operations;
        supervisor.jobs = self.jobs;
        supervisor.graph = self.graph;
        supervisor.control = self.control;
        supervisor.cgroup_cleanup = self.cgroup_cleanup;
        supervisor.start = self.start;
        supervisor.health = self.health;
        supervisor.watchdog = self.watchdog;
        supervisor.control_security = self.control_security;
        supervisor.control_limits = self.control_limits;
        supervisor.jobs_limits = self.jobs_limits;
        supervisor.log_config = self.log_config;
        supervisor.fd_store = fd_store;
        supervisor.relationships = self.relationships;
        supervisor.global_environment = self.global_environment;
        supervisor.eventd_log_socket_path = self.eventd_log_socket_path;
        supervisor.operation_ids = self.operation_ids;
        supervisor.job_ids = self.job_ids;
        supervisor.pending_launches = self.pending_launches;
        supervisor.pending_start_hook_launches = self.pending_start_hook_launches;
        supervisor.pending_post_hook_launches = self.pending_post_hook_launches;
        supervisor.pending_control_launches = self.pending_control_launches;
        supervisor.pending_health_launches = self.pending_health_launches;
        supervisor.submitted = self.submitted;
        supervisor.pending_submitted_launches = self.pending_submitted_launches;
        supervisor.pending_process_setups = self.pending_process_setups;
        supervisor.pending_control_operations = self.pending_control_operations;
        supervisor.retained_service_launches = self.retained_service_launches;
        supervisor.boot_settle = self.boot_settle;
        supervisor.boot_success = self.boot_success;
        supervisor.shutdown = self.shutdown;
        supervisor.shutdown_signals = self.shutdown_signals;
    }

    pub fn shutdown(&self) -> Result<&ShutdownRuntime, ShutdownError> {
        self.shutdown
            .as_ref()
            .ok_or(ShutdownError::NoShutdownInProgress)
    }

    pub fn shutdown_mut(&mut self) -> Result<&mut ShutdownRuntime, ShutdownError> {
        self.shutdown
            .as_mut()
            .ok_or(ShutdownError::NoShutdownInProgress)
    }

    pub fn queue_start_dispatches(
        &mut self,
        dispatches: &[crate::execution::start::StartExecutionDispatch],
    ) {
        for dispatch in dispatches {
            self.queue_start_dispatch(dispatch.job_id, dispatch.job_kind);
        }
    }

    pub fn queue_restart_start_dispatches(
        &mut self,
        dispatches: &[crate::execution::start::RestartStartExecutionDispatch],
    ) {
        for dispatch in dispatches {
            self.queue_start_dispatch(dispatch.job_id, dispatch.job_kind);
        }
    }

    pub fn queue_created_start_job(&mut self, event: &crate::job::JobEvent) {
        match event.job_type {
            crate::job::JobType::ServiceMain => self.pending_launches.push_back(event.job_id),
            crate::job::JobType::PreExecHook => {
                self.pending_start_hook_launches.push_back(event.job_id);
            }
            _ => {}
        }
    }

    pub fn queue_created_post_hook_job(&mut self, event: &crate::job::JobEvent) {
        if event.job_type == crate::job::JobType::PostExecHook {
            self.pending_post_hook_launches.push_back(event.job_id);
        }
    }

    pub fn queue_created_health_check_job(&mut self, event: &crate::job::JobEvent) {
        if event.job_type == crate::job::JobType::HealthCheck {
            self.pending_health_launches.push_back(event.job_id);
        }
    }

    pub fn record_pending_process_setup(&mut self, setup: PendingLaunchSetup) -> Option<i32> {
        let fd = setup.setup_status_fd()?;
        self.pending_process_setups.insert(fd, setup);
        Some(fd)
    }

    fn queue_start_dispatch(&mut self, job_id: JobId, job_kind: StartExecutionJobKind) {
        match job_kind {
            StartExecutionJobKind::ServiceMain => self.pending_launches.push_back(job_id),
            StartExecutionJobKind::PreStartHook { .. } => {
                self.pending_start_hook_launches.push_back(job_id);
            }
        }
    }

    pub fn event_context_ids(events: &[GraphExecutionEvent]) -> BTreeSet<GraphContextId> {
        events.iter().map(|event| event.context_id).collect()
    }
}
