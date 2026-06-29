use std::os::fd::{AsRawFd, OwnedFd};

use crate::boot::phase2::Phase2BootSettings;
use crate::boundary::{BoundaryError, RegistryClient};
use crate::control::socket::LinuxControlSocket;
use crate::supervisor::{Supervisor, SupervisorError};

pub const DEFAULT_BOOT_ATTEMPT_THRESHOLD: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitConfig {
    pub phase2: Phase2BootSettings,
    pub boot_attempt_threshold: u32,
}

impl Default for InitConfig {
    fn default() -> Self {
        Self {
            phase2: Phase2BootSettings::default(),
            boot_attempt_threshold: DEFAULT_BOOT_ATTEMPT_THRESHOLD,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KernelCommandLine {
    pub safe_mode: bool,
    pub recovery: bool,
    pub console: bool,
}

impl KernelCommandLine {
    pub fn parse(contents: &str) -> Self {
        let mut command_line = Self::default();
        for token in contents.split_whitespace() {
            match token {
                "peios.safemode=1" => command_line.safe_mode = true,
                "peios.recovery=1" => command_line.recovery = true,
                // Bring-up/debug affordance: inject the compiled-in console
                // service (a SYSTEM shell attached to /dev/console, respawned on
                // exit) into the Phase 2 boot set. Off by default so a normal
                // image never auto-spawns a privileged console shell.
                "peios.console=1" => command_line.console = true,
                _ => {}
            }
        }
        command_line
    }
}

pub trait InitPlatform {
    fn assert_pid1(&mut self) -> Result<(), InitFatalError>;
    fn read_kernel_command_line(&mut self) -> Result<KernelCommandLine, BoundaryError>;
    fn read_boot_attempt_counter(&mut self) -> Result<u32, BoundaryError>;
    fn verify_root_writable(&mut self) -> Result<(), BoundaryError>;
    fn increment_boot_attempt_counter(&mut self) -> Result<(), BoundaryError>;
    fn mount_virtual_filesystems(&mut self) -> Result<(), BoundaryError>;
    fn set_clock_from_rtc(&mut self) -> Result<(), BoundaryError>;
    fn start_registryd(
        &mut self,
        supervisor: &mut Supervisor,
        registry: &mut dyn RegistryClient,
        observed_at_ns: u64,
    ) -> Result<(), BoundaryError>;
    fn setup_infrastructure(&mut self) -> Result<Phase1Infrastructure, BoundaryError>;
    /// Run the image's autorun scripts (`/usr/system/libexec/autorun.d`),
    /// between base provisioning and Phase-2 service enumeration. peinit is a
    /// generic runner here — it knows nothing about what the scripts do (the
    /// registry-seed apply is just one of them). Scripts own their own lifecycle
    /// (persistent, idempotent, or self-deleting via `rm "$0"`). Fail-open: the
    /// implementation logs its own console summary and never aborts boot. Default
    /// is a no-op so non-Linux platforms and test doubles need no implementation.
    fn run_autorun_scripts(&mut self) -> Result<(), BoundaryError> {
        Ok(())
    }
    fn log_phase1_warning(
        &mut self,
        _warning: &Phase1InfrastructureWarning,
    ) -> Result<(), BoundaryError> {
        Ok(())
    }
    fn write_console_message(&mut self, _message: &str) -> Result<(), BoundaryError> {
        Ok(())
    }
    fn emit_kmes_event(
        &mut self,
        _event: &crate::boundary::KmesEvent,
    ) -> Result<(), BoundaryError> {
        Ok(())
    }
    fn run_recovery_forever(&mut self, reason: &InitRecoveryReason) -> Result<(), BoundaryError>;
}

pub trait InitRuntime {
    fn enter_runtime(
        &mut self,
        supervisor: Supervisor,
        infrastructure: Phase1Infrastructure,
    ) -> Result<(), BoundaryError>;
}

#[derive(Debug, Default)]
pub struct Phase1Infrastructure {
    control_socket: Option<LinuxControlSocket>,
    jfs_device: Option<Phase1JfsDevice>,
    warnings: Vec<Phase1InfrastructureWarning>,
}

impl Phase1Infrastructure {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_jfs_device(jfs_device: Phase1JfsDevice) -> Self {
        Self {
            control_socket: None,
            jfs_device: Some(jfs_device),
            warnings: Vec::new(),
        }
    }

    pub fn control_socket(&self) -> Option<&LinuxControlSocket> {
        self.control_socket.as_ref()
    }

    pub fn jfs_device(&self) -> Option<&Phase1JfsDevice> {
        self.jfs_device.as_ref()
    }

    pub fn set_control_socket(&mut self, control_socket: LinuxControlSocket) {
        self.control_socket = Some(control_socket);
    }

    pub fn warnings(&self) -> &[Phase1InfrastructureWarning] {
        &self.warnings
    }

    pub fn push_warning(&mut self, warning: Phase1InfrastructureWarning) {
        self.warnings.push(warning);
    }

    #[cfg_attr(not(feature = "peios-boundary"), allow(dead_code))]
    pub(crate) fn take_jfs_device(&mut self) -> Option<Phase1JfsDevice> {
        self.jfs_device.take()
    }

    #[cfg_attr(not(feature = "peios-boundary"), allow(dead_code))]
    pub(crate) fn take_control_socket(&mut self) -> Option<LinuxControlSocket> {
        self.control_socket.take()
    }
}

#[derive(Debug)]
pub struct Phase1JfsDevice {
    fd: OwnedFd,
    path: String,
}

impl Phase1JfsDevice {
    pub fn new(fd: OwnedFd, path: impl Into<String>) -> Self {
        Self {
            fd,
            path: path.into(),
        }
    }

    pub fn as_raw_fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase1InfrastructureWarning {
    JfsDeviceOpen { path: String, message: String },
    LoopbackBringUp { interface: String, message: String },
}

impl std::fmt::Display for Phase1InfrastructureWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JfsDeviceOpen { path, message } => {
                write!(f, "JFS device {path} open failed: {message}")
            }
            Self::LoopbackBringUp { interface, message } => {
                write!(
                    f,
                    "loopback interface {interface} bring-up failed: {message}"
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitFatalError {
    NotPid1 { pid: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitRecoveryReason {
    KernelCommandLine(BoundaryError),
    BootAttemptCounter(BoundaryError),
    ForcedByKernelCommandLine,
    BootAttemptThresholdReached { counter: u32, threshold: u32 },
    RootWritable(BoundaryError),
    VirtualFilesystems(BoundaryError),
    RtcClock(BoundaryError),
    Registryd(BoundaryError),
    Infrastructure(BoundaryError),
    Phase2(SupervisorError),
    Runtime(BoundaryError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitRunResult {
    RuntimeReturned,
    RecoveryReturned { reason: InitRecoveryReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitRunError {
    Fatal(InitFatalError),
    RecoveryFailed {
        reason: Box<InitRecoveryReason>,
        error: BoundaryError,
    },
}
