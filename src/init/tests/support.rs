use std::os::fd::{FromRawFd, OwnedFd};

use crate::boot::BootMode;
use crate::boundary::{BoundaryError, Clock, KmesEvent, RegistryClient};
use crate::init::{
    InitFatalError, InitPlatform, InitRecoveryReason, InitRuntime, KernelCommandLine,
    MachineIdStatus, Phase1Infrastructure, Phase1InfrastructureWarning, Phase1JfsDevice,
};
use crate::provisioning::{
    ProvisionedPath, ProvisionedPathApplyReport, ProvisionedPathRegistrySnapshot,
    ProvisionedPathRegistryWarning,
};
use crate::service::runtime::ServiceState;
use crate::service::{Readiness, ServiceDefinition};
use crate::supervisor::Supervisor;

#[derive(Debug)]
pub(super) struct Platform {
    pid1: Result<(), InitFatalError>,
    command_line: Result<KernelCommandLine, BoundaryError>,
    boot_attempt_counter: Result<u32, BoundaryError>,
    increment_result: Result<(), BoundaryError>,
    root_result: Result<(), BoundaryError>,
    mount_result: Result<(), BoundaryError>,
    random_seed_result: Result<bool, BoundaryError>,
    machine_id_result: Result<MachineIdStatus, BoundaryError>,
    rtc_result: Result<(), BoundaryError>,
    registryd_result: Result<(), BoundaryError>,
    infrastructure_result: Result<Phase1Infrastructure, BoundaryError>,
    provision_report: Result<ProvisionedPathApplyReport, BoundaryError>,
    pub(super) provisioned_paths_seen: Vec<ProvisionedPath>,
    pub(super) root_verified: bool,
    pub(super) increment_calls: usize,
    pub(super) warning_logs: Vec<Phase1InfrastructureWarning>,
    pub(super) console_messages: Vec<String>,
    pub(super) recovery_reasons: Vec<InitRecoveryReason>,
    pub(super) kmes_events: Vec<KmesEvent>,
}

impl Platform {
    pub(super) fn new() -> Self {
        Self {
            pid1: Ok(()),
            command_line: Ok(KernelCommandLine::default()),
            boot_attempt_counter: Ok(0),
            increment_result: Ok(()),
            root_result: Ok(()),
            mount_result: Ok(()),
            random_seed_result: Ok(false),
            machine_id_result: Ok(MachineIdStatus::Existing),
            rtc_result: Ok(()),
            registryd_result: Ok(()),
            infrastructure_result: Ok(Phase1Infrastructure::new()),
            provision_report: Ok(ProvisionedPathApplyReport::default()),
            provisioned_paths_seen: Vec::new(),
            root_verified: false,
            increment_calls: 0,
            warning_logs: Vec::new(),
            console_messages: Vec::new(),
            recovery_reasons: Vec::new(),
            kmes_events: Vec::new(),
        }
    }

    pub(super) fn not_pid1(mut self, pid: u32) -> Self {
        self.pid1 = Err(InitFatalError::NotPid1 { pid });
        self
    }

    pub(super) fn command_line(mut self, command_line: KernelCommandLine) -> Self {
        self.command_line = Ok(command_line);
        self
    }

    pub(super) fn boot_attempt_counter(mut self, counter: u32) -> Self {
        self.boot_attempt_counter = Ok(counter);
        self
    }

    pub(super) fn boot_attempt_counter_error(mut self, message: &str) -> Self {
        self.boot_attempt_counter = Err(BoundaryError::Recovery(message.to_string()));
        self
    }

    pub(super) fn command_line_error(mut self, message: &str) -> Self {
        self.command_line = Err(BoundaryError::Recovery(message.to_string()));
        self
    }

    pub(super) fn increment_boot_attempt_counter_error(mut self, message: &str) -> Self {
        self.increment_result = Err(BoundaryError::Recovery(message.to_string()));
        self
    }

    pub(super) fn random_seed_error(mut self, message: &str) -> Self {
        self.random_seed_result = Err(BoundaryError::Recovery(message.to_string()));
        self
    }

    pub(super) fn random_seed_restored(mut self) -> Self {
        self.random_seed_result = Ok(true);
        self
    }

    pub(super) fn machine_id_error(mut self, message: &str) -> Self {
        self.machine_id_result = Err(BoundaryError::Recovery(message.to_string()));
        self
    }

    pub(super) fn machine_id_status(mut self, status: MachineIdStatus) -> Self {
        self.machine_id_result = Ok(status);
        self
    }

    pub(super) fn with_jfs_infrastructure(mut self) -> Self {
        self.infrastructure_result = Ok(Phase1Infrastructure::with_jfs_device(
            Phase1JfsDevice::new(pipe_read_fd(), "/dev/jfs"),
        ));
        self
    }

    pub(super) fn infrastructure(mut self, infrastructure: Phase1Infrastructure) -> Self {
        self.infrastructure_result = Ok(infrastructure);
        self
    }

    pub(super) fn provision_report(mut self, report: ProvisionedPathApplyReport) -> Self {
        self.provision_report = Ok(report);
        self
    }
}

impl InitPlatform for Platform {
    fn assert_pid1(&mut self) -> Result<(), InitFatalError> {
        self.pid1.clone()
    }

    fn read_kernel_command_line(&mut self) -> Result<KernelCommandLine, BoundaryError> {
        self.command_line.clone()
    }

    fn read_boot_attempt_counter(&mut self) -> Result<u32, BoundaryError> {
        self.boot_attempt_counter.clone()
    }

    fn verify_root_writable(&mut self) -> Result<(), BoundaryError> {
        self.root_verified = true;
        self.root_result.clone()
    }

    fn increment_boot_attempt_counter(&mut self) -> Result<(), BoundaryError> {
        self.increment_calls += 1;
        self.increment_result.clone()
    }

    fn mount_virtual_filesystems(&mut self) -> Result<(), BoundaryError> {
        self.mount_result.clone()
    }

    fn restore_random_seed(&mut self) -> Result<bool, BoundaryError> {
        self.random_seed_result.clone()
    }

    fn ensure_machine_id(&mut self) -> Result<MachineIdStatus, BoundaryError> {
        self.machine_id_result.clone()
    }

    fn set_clock_from_rtc(&mut self) -> Result<(), BoundaryError> {
        self.rtc_result.clone()
    }

    fn start_registryd(
        &mut self,
        _supervisor: &mut Supervisor,
        _registry: &mut dyn RegistryClient,
        _observed_at_ns: u64,
    ) -> Result<(), BoundaryError> {
        self.registryd_result.clone()
    }

    fn setup_infrastructure(&mut self) -> Result<Phase1Infrastructure, BoundaryError> {
        std::mem::replace(
            &mut self.infrastructure_result,
            Ok(Phase1Infrastructure::new()),
        )
    }

    fn provision_boot_paths(
        &mut self,
        paths: &[ProvisionedPath],
    ) -> Result<ProvisionedPathApplyReport, BoundaryError> {
        self.provisioned_paths_seen = paths.to_vec();
        self.provision_report.clone()
    }

    fn log_phase1_warning(
        &mut self,
        warning: &Phase1InfrastructureWarning,
    ) -> Result<(), BoundaryError> {
        self.warning_logs.push(warning.clone());
        Ok(())
    }

    fn write_console_message(&mut self, message: &str) -> Result<(), BoundaryError> {
        self.console_messages.push(message.to_string());
        Ok(())
    }

    fn emit_kmes_event(&mut self, event: &KmesEvent) -> Result<(), BoundaryError> {
        self.kmes_events.push(event.clone());
        Ok(())
    }

    fn run_recovery_forever(&mut self, reason: &InitRecoveryReason) -> Result<(), BoundaryError> {
        self.recovery_reasons.push(reason.clone());
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(super) struct Registry {
    services: Vec<ServiceDefinition>,
    provisioned_paths: Result<ProvisionedPathRegistrySnapshot, BoundaryError>,
}

impl Registry {
    pub(super) fn with_services<const N: usize>(services: [ServiceDefinition; N]) -> Self {
        Self {
            services: services.into_iter().collect(),
            provisioned_paths: Ok(ProvisionedPathRegistrySnapshot::empty()),
        }
    }

    pub(super) fn with_provisioned_paths(
        mut self,
        snapshot: ProvisionedPathRegistrySnapshot,
    ) -> Self {
        self.provisioned_paths = Ok(snapshot);
        self
    }

    pub(super) fn provisioned_path_registry_warning(mut self, entry: &str, message: &str) -> Self {
        let mut snapshot = ProvisionedPathRegistrySnapshot::empty();
        snapshot.warnings.push(ProvisionedPathRegistryWarning {
            entry: entry.to_string(),
            message: message.to_string(),
        });
        self.provisioned_paths = Ok(snapshot);
        self
    }
}

impl RegistryClient for Registry {
    fn read_service_definitions(&mut self) -> Result<Vec<ServiceDefinition>, BoundaryError> {
        Ok(self.services.clone())
    }

    fn read_provisioned_paths(&mut self) -> Result<ProvisionedPathRegistrySnapshot, BoundaryError> {
        self.provisioned_paths.clone()
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ClockAt(pub(super) u64);

impl Clock for ClockAt {
    fn monotonic_ns(&mut self) -> Result<u64, BoundaryError> {
        Ok(self.0)
    }
}

#[derive(Debug, Default)]
pub(super) struct Runtime {
    pub(super) entered: bool,
    fail: bool,
    pub(super) received_jfs: bool,
    pub(super) supervisor_mode: Option<BootMode>,
    pub(super) service_names: Vec<String>,
    service_states: Vec<(String, ServiceState)>,
}

impl Runtime {
    pub(super) fn fail_runtime(mut self) -> Self {
        self.fail = true;
        self
    }

    pub(super) fn state_of(&self, service: &str) -> Option<ServiceState> {
        self.service_states
            .iter()
            .find(|(name, _)| name == service)
            .map(|(_, state)| *state)
    }
}

impl InitRuntime for Runtime {
    fn enter_runtime(
        &mut self,
        supervisor: Supervisor,
        infrastructure: Phase1Infrastructure,
    ) -> Result<(), BoundaryError> {
        self.entered = true;
        self.received_jfs = infrastructure.jfs_device().is_some();
        self.supervisor_mode = Some(supervisor.settings().phase2.mode);
        self.service_names = supervisor
            .services()
            .service_names()
            .into_iter()
            .map(ToString::to_string)
            .collect();
        self.service_states = self
            .service_names
            .iter()
            .map(|service| {
                (
                    service.clone(),
                    supervisor.service_status(service).expect("status").state,
                )
            })
            .collect();
        if self.fail {
            Err(BoundaryError::Recovery("runtime failed".to_string()))
        } else {
            Ok(())
        }
    }
}

pub(super) fn service(name: &str) -> ServiceDefinition {
    let mut service = ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"));
    service.readiness = Readiness::Alive;
    service
}

pub(super) fn critical_service(name: &str) -> ServiceDefinition {
    let mut service = service(name);
    service.error_control = crate::service::ErrorControl::Critical;
    service.safe_mode = true;
    service
}

fn pipe_read_fd() -> OwnedFd {
    let mut fds = [0_i32; 2];
    let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    assert_eq!(
        result,
        0,
        "pipe2 failed: {}",
        std::io::Error::last_os_error()
    );
    unsafe {
        libc::close(fds[1]);
        OwnedFd::from_raw_fd(fds[0])
    }
}
