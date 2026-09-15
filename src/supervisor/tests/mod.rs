mod boot_launch;
mod conditions;
mod control_commands;
mod health;
mod launch_failure;
mod lifecycle_commands;
mod notify;
mod on_demand_start;
mod operation_maintenance;
mod phase1_registryd;
mod post_start_hooks;
mod queued_operations;
mod reaped_before_setup;
mod relationships;
mod restart;
mod shutdown;
mod stale_launch_queue;
mod start_hooks;
mod submitted;
mod terminal_release;
mod timer;
mod tty_arbitration;
mod undecodable;
mod watchdog;

use std::collections::{BTreeMap, VecDeque};

use crate::boot::BootMode;
use crate::boot::phase2::Phase2BootSettings;
use crate::boundary::{
    BoundaryError, CgroupRemoveOutcome, Clock, FilesystemCheckHelperLauncher,
    FilesystemCheckHelperReader, FilesystemCheckHelperRequest, FilesystemCheckReport,
    LaunchedFilesystemCheckHelper, LaunchedProcess, ProcessController, ProcessLaunchSpec,
    ProcessLauncher, ProcessSignal, ProcessTarget, RealtimeClock, RegistryClient, TokenHandle,
    TokenProvider,
};
use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::execution::launch::{LISTEN_FDNAMES, LISTEN_FDS, NOTIFY_SOCKET, PATH};
use crate::job::{JobRecord, JobState};
use crate::security::TokenSummary;
use crate::service::{Readiness, ServiceDefinition, ServiceEnvironmentVariable, ServiceType};

const BOOT_NS: u64 = 1_000_000_000;
const AUTHD_LAUNCH_NS: u64 = 1_000_010_000;
const APP_LAUNCH_NS: u64 = 1_000_020_000;
const MIGRATE_LAUNCH_NS: u64 = 1_000_012_000;
const MIGRATE_DONE_NS: u64 = 1_000_014_000;
const ADMIN_START_NS: u64 = 1_500_000_000;
const DB_LAUNCH_NS: u64 = 1_500_010_000;
const ON_DEMAND_APP_LAUNCH_NS: u64 = 1_500_020_000;
const LIFECYCLE_COMMAND_NS: u64 = 1_600_000_000;
const APP_CRASH_NS: u64 = 2_000_000_000;
const RESTART_LAUNCH_NS: u64 = 3_000_010_000;
const TEST_REALTIME_NS: u64 = 1_717_171_717_123_456_789;

#[derive(Debug, Clone)]
struct StaticRegistry {
    services: Vec<ServiceDefinition>,
    global_environment: Vec<ServiceEnvironmentVariable>,
    control_security: ControlSecurityDescriptor,
    control_limits: ControlSocketLimits,
    shutdown_timeout_secs: Option<u32>,
    eventd_log_socket_path: Option<String>,
    /// Keys the partial boot read reports as present but undecodable.
    undecodable: Vec<crate::boundary::UndecodableService>,
}

impl StaticRegistry {
    fn services(services: Vec<ServiceDefinition>) -> Self {
        Self {
            services,
            global_environment: Vec::new(),
            control_security: ControlSecurityDescriptor::Default,
            control_limits: ControlSocketLimits::default(),
            shutdown_timeout_secs: None,
            eventd_log_socket_path: None,
            undecodable: Vec::new(),
        }
    }

    fn with_undecodable(mut self, name: &str, message: &str) -> Self {
        self.undecodable
            .push(crate::boundary::UndecodableService {
                name: name.to_string(),
                message: message.to_string(),
            });
        self
    }

    fn services_with_global_environment(
        services: Vec<ServiceDefinition>,
        global_environment: Vec<ServiceEnvironmentVariable>,
    ) -> Self {
        Self {
            services,
            global_environment,
            control_security: ControlSecurityDescriptor::Default,
            control_limits: ControlSocketLimits::default(),
            shutdown_timeout_secs: None,
            eventd_log_socket_path: None,
            undecodable: Vec::new(),
        }
    }

    fn with_control_config(
        mut self,
        control_security: ControlSecurityDescriptor,
        control_limits: ControlSocketLimits,
    ) -> Self {
        self.control_security = control_security;
        self.control_limits = control_limits;
        self
    }

    fn with_eventd_log_socket_path(mut self, path: impl Into<String>) -> Self {
        self.eventd_log_socket_path = Some(path.into());
        self
    }

    fn with_shutdown_timeout_secs(mut self, timeout_secs: u32) -> Self {
        self.shutdown_timeout_secs = Some(timeout_secs);
        self
    }
}

impl RegistryClient for StaticRegistry {
    fn read_service_definitions(&mut self) -> Result<Vec<ServiceDefinition>, BoundaryError> {
        Ok(self.services.clone())
    }

    fn read_service_definitions_partial(
        &mut self,
    ) -> Result<crate::boundary::ServiceDefinitionsRead, BoundaryError> {
        Ok(crate::boundary::ServiceDefinitionsRead {
            definitions: self.services.clone(),
            undecodable: self.undecodable.clone(),
        })
    }

    fn read_global_environment(
        &mut self,
    ) -> Result<Vec<ServiceEnvironmentVariable>, BoundaryError> {
        Ok(self.global_environment.clone())
    }

    fn read_control_security(&mut self) -> Result<ControlSecurityDescriptor, BoundaryError> {
        Ok(self.control_security.clone())
    }

    fn read_control_socket_limits(&mut self) -> Result<ControlSocketLimits, BoundaryError> {
        Ok(self.control_limits)
    }

    fn read_shutdown_timeout_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(self.shutdown_timeout_secs)
    }

    fn read_eventd_log_socket_path(&mut self) -> Result<Option<String>, BoundaryError> {
        Ok(self.eventd_log_socket_path.clone())
    }
}

#[derive(Debug, Clone)]
struct ScriptedClock {
    times: VecDeque<u64>,
}

impl ScriptedClock {
    fn new(times: impl Into<VecDeque<u64>>) -> Self {
        Self {
            times: times.into(),
        }
    }
}

impl Clock for ScriptedClock {
    fn monotonic_ns(&mut self) -> Result<u64, BoundaryError> {
        self.times
            .pop_front()
            .ok_or_else(|| BoundaryError::Clock("scripted clock exhausted".to_string()))
    }
}

impl RealtimeClock for ScriptedClock {
    fn realtime_ns(&mut self) -> Result<u64, BoundaryError> {
        Ok(TEST_REALTIME_NS)
    }
}

#[derive(Debug, Default)]
struct TestTokenProvider {
    observed_jobs: Vec<String>,
}

impl TokenProvider for TestTokenProvider {
    fn materialize_service_token(&mut self, job: &JobRecord) -> Result<TokenHandle, BoundaryError> {
        self.observed_jobs.push(job_name(job));
        Ok(TokenHandle {
            fd: 8,
            identity: job.resolved_identity.clone(),
            summary: TokenSummary::requested_identity(job.resolved_identity.clone()),
        })
    }
}

#[derive(Debug)]
struct TestProcessLauncher {
    results: VecDeque<Result<LaunchedProcess, BoundaryError>>,
    runtime_directory_results: VecDeque<Result<(), BoundaryError>>,
    observed_jobs: Vec<String>,
    observed_runtime_directory_services: Vec<String>,
    observed_runtime_directories: Vec<Vec<String>>,
    observed_notify_sockets: Vec<Option<String>>,
    observed_paths: Vec<Option<String>>,
    observed_app_modes: Vec<Option<String>>,
    observed_global_only: Vec<Option<String>>,
    observed_listen_fds: Vec<Option<String>>,
    observed_listen_fdnames: Vec<Option<String>>,
    observed_inherited_fd_names: Vec<Vec<String>>,
    observed_setup_timeouts: Vec<u64>,
}

#[derive(Debug)]
struct TestFilesystemCheckLauncher {
    requests: Vec<FilesystemCheckHelperRequest>,
    result_fd: i32,
    pidfd: i32,
    pid: u32,
}

#[derive(Debug, Default)]
struct TestFilesystemCheckReader {
    helpers: Vec<LaunchedFilesystemCheckHelper>,
    reports: VecDeque<Result<Option<FilesystemCheckReport>, BoundaryError>>,
    released_fds: Vec<(i32, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedSignal {
    target: ProcessTarget,
    signal: ProcessSignal,
}

#[derive(Debug, Default)]
struct TestProcessController {
    pidfd_match_checks: Vec<(i32, u32)>,
    pidfd_matches: BTreeMap<(i32, u32), bool>,
    signals: Vec<ObservedSignal>,
    cgroup_kills: Vec<String>,
    cgroup_populated: BTreeMap<String, bool>,
    cgroup_populated_checks: Vec<String>,
    cgroup_removes: Vec<String>,
    cgroup_remove_results: BTreeMap<String, CgroupRemoveOutcome>,
}

impl ProcessController for TestProcessController {
    fn pidfd_matches_pid(&mut self, pidfd: i32, pid: u32) -> Result<bool, BoundaryError> {
        self.pidfd_match_checks.push((pidfd, pid));
        Ok(self
            .pidfd_matches
            .get(&(pidfd, pid))
            .copied()
            .unwrap_or(true))
    }

    fn signal_main(
        &mut self,
        target: &ProcessTarget,
        signal: ProcessSignal,
    ) -> Result<(), BoundaryError> {
        self.signals.push(ObservedSignal {
            target: target.clone(),
            signal,
        });
        Ok(())
    }

    fn kill_cgroup(&mut self, cgroup_id: &str) -> Result<(), BoundaryError> {
        self.cgroup_kills.push(cgroup_id.to_string());
        Ok(())
    }

    fn cgroup_populated(&mut self, cgroup_id: &str) -> Result<bool, BoundaryError> {
        self.cgroup_populated_checks.push(cgroup_id.to_string());
        Ok(self
            .cgroup_populated
            .get(cgroup_id)
            .copied()
            .unwrap_or(true))
    }

    fn remove_cgroup(&mut self, cgroup_id: &str) -> Result<CgroupRemoveOutcome, BoundaryError> {
        self.cgroup_removes.push(cgroup_id.to_string());
        Ok(self
            .cgroup_remove_results
            .get(cgroup_id)
            .copied()
            .unwrap_or(CgroupRemoveOutcome::Removed))
    }
}

impl TestProcessController {
    fn set_pidfd_match(&mut self, pidfd: i32, pid: u32, matches: bool) {
        self.pidfd_matches.insert((pidfd, pid), matches);
    }

    fn set_cgroup_populated(&mut self, cgroup_id: impl Into<String>, populated: bool) {
        self.cgroup_populated.insert(cgroup_id.into(), populated);
    }

    fn set_cgroup_remove_result(
        &mut self,
        cgroup_id: impl Into<String>,
        outcome: CgroupRemoveOutcome,
    ) {
        self.cgroup_remove_results.insert(cgroup_id.into(), outcome);
    }
}

impl TestProcessLauncher {
    fn new(processes: Vec<LaunchedProcess>) -> Self {
        Self {
            results: processes.into_iter().map(Ok).collect(),
            runtime_directory_results: VecDeque::new(),
            observed_jobs: Vec::new(),
            observed_runtime_directory_services: Vec::new(),
            observed_runtime_directories: Vec::new(),
            observed_notify_sockets: Vec::new(),
            observed_paths: Vec::new(),
            observed_app_modes: Vec::new(),
            observed_global_only: Vec::new(),
            observed_listen_fds: Vec::new(),
            observed_listen_fdnames: Vec::new(),
            observed_inherited_fd_names: Vec::new(),
            observed_setup_timeouts: Vec::new(),
        }
    }

    fn results(results: Vec<Result<LaunchedProcess, BoundaryError>>) -> Self {
        Self {
            results: results.into(),
            runtime_directory_results: VecDeque::new(),
            observed_jobs: Vec::new(),
            observed_runtime_directory_services: Vec::new(),
            observed_runtime_directories: Vec::new(),
            observed_notify_sockets: Vec::new(),
            observed_paths: Vec::new(),
            observed_app_modes: Vec::new(),
            observed_global_only: Vec::new(),
            observed_listen_fds: Vec::new(),
            observed_listen_fdnames: Vec::new(),
            observed_inherited_fd_names: Vec::new(),
            observed_setup_timeouts: Vec::new(),
        }
    }

    fn runtime_directory_results(mut self, results: Vec<Result<(), BoundaryError>>) -> Self {
        self.runtime_directory_results = results.into();
        self
    }
}

impl ProcessLauncher for TestProcessLauncher {
    fn provision_service_runtime_directories(
        &mut self,
        service: &ServiceDefinition,
    ) -> Result<(), BoundaryError> {
        self.observed_runtime_directory_services
            .push(service.name.clone());
        self.observed_runtime_directories.push(
            service
                .runtime_directories
                .iter()
                .map(|directory| directory.name.clone())
                .collect(),
        );
        self.runtime_directory_results.pop_front().unwrap_or(Ok(()))
    }

    fn launch_service(
        &mut self,
        spec: ProcessLaunchSpec<'_>,
    ) -> Result<LaunchedProcess, BoundaryError> {
        self.observed_jobs.push(job_name(spec.job));
        self.observed_notify_sockets.push(
            spec.environment_value(NOTIFY_SOCKET)
                .map(ToString::to_string),
        );
        self.observed_paths
            .push(spec.environment_value(PATH).map(ToString::to_string));
        self.observed_app_modes
            .push(spec.environment_value("APP_MODE").map(ToString::to_string));
        self.observed_global_only.push(
            spec.environment_value("GLOBAL_ONLY")
                .map(ToString::to_string),
        );
        self.observed_listen_fds
            .push(spec.environment_value(LISTEN_FDS).map(ToString::to_string));
        self.observed_listen_fdnames.push(
            spec.environment_value(LISTEN_FDNAMES)
                .map(ToString::to_string),
        );
        self.observed_inherited_fd_names.push(
            spec.inherited_fds
                .iter()
                .map(|fd| fd.name.clone())
                .collect(),
        );
        self.observed_setup_timeouts.push(spec.setup_timeout_secs);
        self.results
            .pop_front()
            .ok_or_else(|| BoundaryError::Process("scripted launcher exhausted".to_string()))
            .and_then(|result| result)
    }
}

impl Default for TestFilesystemCheckLauncher {
    fn default() -> Self {
        Self {
            requests: Vec::new(),
            result_fd: 81,
            pidfd: 80,
            pid: 8001,
        }
    }
}

impl TestFilesystemCheckLauncher {
    fn result_fd(mut self, result_fd: i32) -> Self {
        self.result_fd = result_fd;
        self
    }
}

impl FilesystemCheckHelperLauncher for TestFilesystemCheckLauncher {
    fn launch_filesystem_check_helper(
        &mut self,
        request: FilesystemCheckHelperRequest,
    ) -> Result<LaunchedFilesystemCheckHelper, BoundaryError> {
        self.requests.push(request.clone());
        Ok(LaunchedFilesystemCheckHelper {
            service: request.service,
            operation_id: request.operation_id,
            checks: request.checks,
            pid: self.pid,
            pidfd: self.pidfd,
            result_fd: self.result_fd,
            cgroup_id: request.cgroup_id,
        })
    }
}

impl TestFilesystemCheckReader {
    fn new(
        reports: impl Into<VecDeque<Result<Option<FilesystemCheckReport>, BoundaryError>>>,
    ) -> Self {
        Self {
            helpers: Vec::new(),
            reports: reports.into(),
            released_fds: Vec::new(),
        }
    }
}

impl FilesystemCheckHelperReader for TestFilesystemCheckReader {
    fn read_filesystem_check_report(
        &mut self,
        helper: &LaunchedFilesystemCheckHelper,
    ) -> Result<Option<FilesystemCheckReport>, BoundaryError> {
        self.helpers.push(helper.clone());
        self.reports.pop_front().unwrap_or(Ok(None))
    }

    fn release_filesystem_check_helper_fds(&mut self, result_fd: i32, pidfd: i32) {
        self.released_fds.push((result_fd, pidfd));
    }
}

fn settings() -> Phase2BootSettings {
    Phase2BootSettings {
        mode: BootMode::Full,
        max_parallel_starts: 10,
        ..Phase2BootSettings::default()
    }
}

fn alive_service(name: &str) -> ServiceDefinition {
    let mut definition = ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"));
    definition.readiness = Readiness::Alive;
    definition
}

fn oneshot_service(name: &str) -> ServiceDefinition {
    let mut definition = ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"));
    definition.service_type = ServiceType::Oneshot;
    definition
}

fn process(pid: u32, pidfd: i32) -> LaunchedProcess {
    LaunchedProcess {
        pid,
        pidfd,
        stdout_fd: None,
        stderr_fd: None,
        setup_status_fd: None,
        cleanup_evidence: Vec::new(),
    }
}

fn job_name(job: &JobRecord) -> String {
    job.service
        .clone()
        .unwrap_or_else(|| format!("{:?}:{:?}", job.id, JobState::Created))
}
