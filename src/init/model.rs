use crate::boot::phase2::Phase2BootSettings;
use crate::boundary::{BoundaryError, RegistryClient};
use crate::control::socket::LinuxControlSocket;
use crate::jobs::socket::LinuxJobsSocket;
use crate::provisioning::{ProvisionedPath, ProvisionedPathApplyReport};
use crate::supervisor::{Supervisor, SupervisorError};

use super::devices::DeviceNodePolicyReport;

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

/// The `peios.*` tokens peinit understands on the kernel command line.
///
/// Everything here is either a per-boot policy decision (which mode to boot)
/// or a Phase-1 value — one peinit needs *before* registryd is serving, so the
/// registry cannot supply it. Anything readable after registryd starts belongs
/// in `Machine\\System\\{Boot,Init}` instead, where it can be inspected,
/// secured and changed without editing a boot entry.
///
/// Notably absent: service selection. `peios.console=1` and `peios.login=1`
/// used to inject compiled-in console/authd/lpsd/login definitions into the
/// boot set. They are ordinary registry services now — presence in
/// `Machine\\System\\Services` is what selects them, the same as everything
/// else peinit starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KernelCommandLine {
    pub safe_mode: bool,
    pub recovery: bool,
    /// `peios.bootattempts=N` — consecutive failed boots before peinit gives up
    /// and enters recovery. Read in Phase 1, before the registry exists, which
    /// is why it is here and not a registry value. `0` disables the check.
    pub boot_attempt_threshold: Option<u32>,
    /// `peios.notifysocket=PATH` — where the sd_notify socket is bound. Phase 1
    /// again: registryd is the first service to use it, and it must be bound
    /// before registryd is launched.
    pub notify_socket_path: Option<String>,
    /// `peios.quiet=N` — how much peinit may write to the console.
    pub quiet: QuietLevel,
}

/// How much peinit may write to `/dev/console`.
///
/// Two independent rules, which is why this is a level rather than a flag:
///
/// - **Terminal ownership.** Once a service holds the console as its
///   controlling terminal, peinit writing there corrupts somebody else's
///   session — its progress lands mid-prompt, and the reader cannot tell input
///   from log. Active at `Standard` and `Blackout`; only `Verbose` turns it
///   off.
/// - **Blackout.** `Blackout` additionally drops ordinary progress everywhere,
///   for an image that wants a silent console.
///
/// They stack rather than scale: errors are never *less* visible at `Blackout`
/// than at `Standard`. An error overrides a blackout — silence was a
/// preference and an error is news — but not terminal ownership, which is not
/// peinit's to override. Only a message that means the machine is about to be
/// lost ([`ConsoleSeverity::Critical`](crate::runtime::ConsoleSeverity)) is
/// worth one corrupted line of someone else's session.
///
/// Suppressed messages are currently dropped rather than kept. That is a known
/// gap: once eventd is in the image they should go there, and until then
/// `peios.quiet=0` is how you get the full narrative back on a machine you are
/// debugging.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QuietLevel {
    /// `0` — write everything, even into a terminal a service owns. The
    /// bring-up setting: peinit's console output is the only diagnostic
    /// channel on an image with no eventd, and a scrambled prompt is a cheap
    /// price for keeping it.
    Verbose,
    /// `1` (default) — stay out of a terminal a service owns.
    #[default]
    Standard,
    /// `2` — as `Standard`, and drop ordinary progress everywhere. Errors still
    /// reach the console; this silences the narrative, not the news.
    Blackout,
}

impl QuietLevel {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "0" => Some(Self::Verbose),
            "1" => Some(Self::Standard),
            "2" => Some(Self::Blackout),
            _ => None,
        }
    }

    /// Whether a service owning the console silences peinit.
    pub fn respects_terminal_ownership(self) -> bool {
        self != Self::Verbose
    }

    /// Whether ordinary progress is dropped regardless of who owns what.
    pub fn suppresses_status(self) -> bool {
        self == Self::Blackout
    }
}

impl KernelCommandLine {
    pub fn parse(contents: &str) -> Self {
        let mut command_line = Self::default();
        for token in contents.split_whitespace() {
            // Last occurrence wins for valued tokens, matching the kernel's own
            // handling of a repeated parameter.
            match token.split_once('=') {
                Some(("peios.safemode", "1")) => command_line.safe_mode = true,
                Some(("peios.recovery", "1")) => command_line.recovery = true,
                Some(("peios.bootattempts", value)) => {
                    // A malformed value leaves the default in place rather than
                    // failing the boot: this parser runs before there is any way
                    // to report a diagnostic, and refusing to boot over a typo
                    // in a tuning knob is a worse outcome than ignoring it.
                    if let Ok(threshold) = value.parse::<u32>() {
                        command_line.boot_attempt_threshold = Some(threshold);
                    }
                }
                Some(("peios.notifysocket", value)) if !value.is_empty() => {
                    command_line.notify_socket_path = Some(value.to_string());
                }
                Some(("peios.quiet", value)) => {
                    // Unparseable leaves the default, like the other valued
                    // tokens: a typo in a logging knob must not decide how the
                    // machine boots.
                    if let Some(level) = QuietLevel::parse(value) {
                        command_line.quiet = level;
                    }
                }
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
    /// Stamp the per-node descriptors on the static `/dev` nodes every
    /// principal must be able to use (`/dev/null` and its kin). Runs once the
    /// virtual filesystems are up. The report is advisory: a node that could
    /// not be stamped stays on the inherited administrators-only default and
    /// the boot goes on. Default is an empty report so non-Linux platforms
    /// and test doubles need no implementation.
    fn apply_device_node_policy(&mut self) -> Result<DeviceNodePolicyReport, BoundaryError> {
        Ok(DeviceNodePolicyReport::default())
    }
    fn restore_random_seed(&mut self) -> Result<bool, BoundaryError> {
        Ok(false)
    }
    fn ensure_machine_id(&mut self) -> Result<MachineIdStatus, BoundaryError> {
        Ok(MachineIdStatus::Existing)
    }
    fn set_clock_from_rtc(&mut self) -> Result<(), BoundaryError>;
    fn start_registryd(
        &mut self,
        supervisor: &mut Supervisor,
        registry: &mut dyn RegistryClient,
        observed_at_ns: u64,
    ) -> Result<(), BoundaryError>;
    fn setup_infrastructure(&mut self) -> Result<Phase1Infrastructure, BoundaryError>;
    /// Run the image's autorun scripts (`/lcl/policy/autorun.d`),
    /// between base provisioning and Phase-2 service enumeration. peinit is a
    /// generic runner here — it knows nothing about what the scripts do (the
    /// registry-seed apply is just one of them). Scripts own their own lifecycle
    /// (persistent, idempotent, or self-deleting via `rm "$0"`). Fail-open: the
    /// implementation logs its own console summary and never aborts boot. Default
    /// is a no-op so non-Linux platforms and test doubles need no implementation.
    fn run_autorun_scripts(&mut self) -> Result<(), BoundaryError> {
        Ok(())
    }
    fn provision_boot_paths(
        &mut self,
        _paths: &[ProvisionedPath],
    ) -> Result<ProvisionedPathApplyReport, BoundaryError> {
        Ok(ProvisionedPathApplyReport::default())
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
    jobs_socket: Option<LinuxJobsSocket>,
    warnings: Vec<Phase1InfrastructureWarning>,
}

impl Phase1Infrastructure {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn control_socket(&self) -> Option<&LinuxControlSocket> {
        self.control_socket.as_ref()
    }

    pub fn set_control_socket(&mut self, control_socket: LinuxControlSocket) {
        self.control_socket = Some(control_socket);
    }

    pub fn jobs_socket(&self) -> Option<&LinuxJobsSocket> {
        self.jobs_socket.as_ref()
    }

    pub fn set_jobs_socket(&mut self, jobs_socket: LinuxJobsSocket) {
        self.jobs_socket = Some(jobs_socket);
    }

    #[cfg_attr(not(feature = "peios-boundary"), allow(dead_code))]
    pub(crate) fn take_jobs_socket(&mut self) -> Option<LinuxJobsSocket> {
        self.jobs_socket.take()
    }

    pub fn warnings(&self) -> &[Phase1InfrastructureWarning] {
        &self.warnings
    }

    pub fn push_warning(&mut self, warning: Phase1InfrastructureWarning) {
        self.warnings.push(warning);
    }

    #[cfg_attr(not(feature = "peios-boundary"), allow(dead_code))]
    pub(crate) fn take_control_socket(&mut self) -> Option<LinuxControlSocket> {
        self.control_socket.take()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase1InfrastructureWarning {
    LoopbackBringUp { interface: String, message: String },
}

impl std::fmt::Display for Phase1InfrastructureWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
    MachineId(BoundaryError),
    RtcClock(BoundaryError),
    Registryd(BoundaryError),
    Provisioning(BoundaryError),
    Infrastructure(BoundaryError),
    Phase2(SupervisorError),
    Runtime(BoundaryError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineIdStatus {
    Existing,
    Generated,
    ReplacedInvalid,
    /// The identifier could not be read or persisted; this boot uses one that
    /// will not survive it. A warning, never a reason to enter recovery
    /// (§2.1) — the machine ID is a local opaque install identifier, not a
    /// credential, and failing a boot over it is disproportionate.
    Ephemeral { reason: String },
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
