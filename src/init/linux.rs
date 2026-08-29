use crate::boundary::{
    BoundaryError, KmesEventSink, LinuxKmesEventSink, LinuxMachineIdStatus, LinuxMonotonicClock,
    LinuxRandomSeedRestoreStatus, ensure_linux_machine_id, provision_linux_boot_paths,
    restore_linux_random_seed,
};
use crate::provisioning::{ProvisionedPath, ProvisionedPathApplyReport};
use crate::registry::LcsRegistryClient;
use crate::runtime::{
    LinuxRuntimeConfig, LinuxRuntimeSetupError, LinuxShutdownRuntime,
};
use crate::supervisor::Supervisor;

use super::{
    DeviceNodePolicyReport, InitConfig, InitFatalError, InitPlatform, InitRecoveryReason,
    InitRuntime, KernelCommandLine, MachineIdStatus, Phase1Infrastructure, run_init,
};

mod autorun;
mod devices;
mod files;
mod infrastructure;
mod loopback;
#[cfg(test)]
mod loopback_tests;
mod mounts;
mod recovery_console;
mod registryd;
mod rtc;

use devices::apply_linux_device_node_policy;
use files::LinuxInitFiles;
use infrastructure::setup_linux_phase1_infrastructure;
use mounts::{LinuxPhase1MountSyscalls, mount_phase1_virtual_filesystems};
use recovery_console::{LinuxRecoveryConsoleBoundary, run_recovery_console_forever, write_console};
use registryd::start_linux_phase1_registryd;
use rtc::{LinuxRtcClockSyscalls, set_clock_from_hardware_rtc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxInitPlatform {
    files: LinuxInitFiles,
}

impl LinuxInitPlatform {
    pub fn new() -> Self {
        Self {
            files: LinuxInitFiles::new(),
        }
    }
}

impl Default for LinuxInitPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl InitPlatform for LinuxInitPlatform {
    fn assert_pid1(&mut self) -> Result<(), InitFatalError> {
        let pid = unsafe { libc::getpid() };
        if pid == 1 {
            Ok(())
        } else {
            Err(InitFatalError::NotPid1 { pid: pid as u32 })
        }
    }

    fn read_kernel_command_line(&mut self) -> Result<KernelCommandLine, BoundaryError> {
        self.files.read_kernel_command_line()
    }

    fn read_boot_attempt_counter(&mut self) -> Result<u32, BoundaryError> {
        self.files.read_boot_attempt_counter()
    }

    fn verify_root_writable(&mut self) -> Result<(), BoundaryError> {
        self.files.verify_root_writable()
    }

    fn increment_boot_attempt_counter(&mut self) -> Result<(), BoundaryError> {
        self.files.increment_boot_attempt_counter()
    }

    fn mount_virtual_filesystems(&mut self) -> Result<(), BoundaryError> {
        let mut syscalls = LinuxPhase1MountSyscalls;
        mount_phase1_virtual_filesystems(self.files.mountinfo_path(), &mut syscalls)
    }

    fn apply_device_node_policy(&mut self) -> Result<DeviceNodePolicyReport, BoundaryError> {
        Ok(apply_linux_device_node_policy())
    }

    fn restore_random_seed(&mut self) -> Result<bool, BoundaryError> {
        match restore_linux_random_seed().map_err(random_seed_restore_error)? {
            LinuxRandomSeedRestoreStatus::Missing => Ok(false),
            LinuxRandomSeedRestoreStatus::Credited => Ok(true),
            LinuxRandomSeedRestoreStatus::MixedWithoutCredit { credit_error } => {
                Err(BoundaryError::Recovery(format!(
                    "random seed mixed without entropy credit: {credit_error}"
                )))
            }
        }
    }

    fn ensure_machine_id(&mut self) -> Result<MachineIdStatus, BoundaryError> {
        match ensure_linux_machine_id().map_err(machine_id_error)? {
            LinuxMachineIdStatus::Existing => Ok(MachineIdStatus::Existing),
            LinuxMachineIdStatus::Generated => Ok(MachineIdStatus::Generated),
            LinuxMachineIdStatus::ReplacedInvalid => Ok(MachineIdStatus::ReplacedInvalid),
        }
    }

    fn set_clock_from_rtc(&mut self) -> Result<(), BoundaryError> {
        let mut syscalls = LinuxRtcClockSyscalls;
        set_clock_from_hardware_rtc(&mut syscalls)
    }

    fn start_registryd(
        &mut self,
        supervisor: &mut Supervisor,
        registry: &mut dyn crate::boundary::RegistryClient,
        observed_at_ns: u64,
    ) -> Result<(), BoundaryError> {
        start_linux_phase1_registryd(supervisor, registry, observed_at_ns)
    }

    fn setup_infrastructure(&mut self) -> Result<Phase1Infrastructure, BoundaryError> {
        setup_linux_phase1_infrastructure()
    }

    fn run_autorun_scripts(&mut self) -> Result<(), BoundaryError> {
        autorun::run_autorun_scripts()
    }

    fn provision_boot_paths(
        &mut self,
        paths: &[ProvisionedPath],
    ) -> Result<ProvisionedPathApplyReport, BoundaryError> {
        Ok(provision_linux_boot_paths(paths))
    }

    fn log_phase1_warning(
        &mut self,
        warning: &super::Phase1InfrastructureWarning,
    ) -> Result<(), BoundaryError> {
        write_console(&format!("peinit warning: {warning}\n"))
    }

    fn write_console_message(&mut self, message: &str) -> Result<(), BoundaryError> {
        write_console(message)
    }

    fn emit_kmes_event(&mut self, event: &crate::boundary::KmesEvent) -> Result<(), BoundaryError> {
        LinuxKmesEventSink::new().emit_kmes_event(event)
    }

    fn run_recovery_forever(&mut self, reason: &InitRecoveryReason) -> Result<(), BoundaryError> {
        let mut console = LinuxRecoveryConsoleBoundary;
        run_recovery_console_forever(&mut console, reason)
    }
}

#[derive(Debug, Default)]
pub struct LinuxRuntimeEntrypoint;

impl InitRuntime for LinuxRuntimeEntrypoint {
    fn enter_runtime(
        &mut self,
        mut supervisor: Supervisor,
        mut infrastructure: Phase1Infrastructure,
    ) -> Result<(), BoundaryError> {
        let mut runtime = LinuxShutdownRuntime::setup_with_infrastructure(
            LinuxRuntimeConfig {
                quiet: supervisor.settings().quiet,
                ..LinuxRuntimeConfig::default()
            },
            &mut infrastructure,
        )
        .map_err(runtime_setup_error)?;
        runtime
            .register_retained_service_launches(&mut supervisor)
            .map_err(|error| {
                BoundaryError::Recovery(format!(
                    "retained service log-pipe registration failed: {error:?}"
                ))
            })?;
        let timer_registration =
            runtime
                .register_calendar_timers(&mut supervisor)
                .map_err(|error| {
                    BoundaryError::Recovery(format!("calendar timer registration failed: {error}"))
                })?;
        log_rejected_calendar_timers(&timer_registration.rejected);
        runtime
            .emit_boot_calendar_timer_turns(&timer_registration.catch_up_turns)
            .map_err(|error| {
                BoundaryError::Recovery(format!(
                    "boot timer catch-up event emission failed: {error:?}"
                ))
            })?;
        runtime
            .run_forever(&mut supervisor)
            .map_err(|error| BoundaryError::Recovery(format!("runtime loop failed: {error:?}")))
    }
}

#[derive(Debug)]
pub enum LinuxInitError {
    Init(Box<super::InitRunError>),
}

pub fn run_linux_peinit() -> Result<super::InitRunResult, LinuxInitError> {
    let mut platform = LinuxInitPlatform::new();
    let mut registry = LcsRegistryClient;
    let mut clock = LinuxMonotonicClock::new();
    let mut runtime = LinuxRuntimeEntrypoint;
    run_init(
        InitConfig::default(),
        &mut platform,
        &mut registry,
        &mut clock,
        &mut runtime,
    )
    .map_err(|error| LinuxInitError::Init(Box::new(error)))
}

fn runtime_setup_error(error: LinuxRuntimeSetupError) -> BoundaryError {
    BoundaryError::Recovery(format!("runtime setup failed: {error:?}"))
}

fn random_seed_restore_error(error: crate::boundary::LinuxRandomSeedError) -> BoundaryError {
    BoundaryError::Recovery(format!("{error:?}"))
}

fn machine_id_error(error: crate::boundary::LinuxMachineIdError) -> BoundaryError {
    BoundaryError::Recovery(format!("{error:?}"))
}

/// Report timers that will not arm.
///
/// A malformed or unsatisfiable schedule used to abort registration of every
/// timer and drop the machine into the recovery console. It now fails only
/// that trigger — which is right, and would be worse than useless if it also
/// happened silently.
fn log_rejected_calendar_timers(rejected: &[crate::timer::boot::TimerBootPlanError]) {
    for error in rejected {
        let _ = write_console(&format!(
            "peinit warning: calendar timer not armed: {error:?}\n"
        ));
    }
}
