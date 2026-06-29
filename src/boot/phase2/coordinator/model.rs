use crate::boot::BootMode;
use crate::boot::phase2::{Phase2BootPlan, Phase2BootPlanError};
use crate::boundary::BoundaryError;
use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::logging::RuntimeLogConfig;
use crate::operation::store::{OperationStoreError, Phase2BootDispatch};
use crate::registry::RegistryConfigWarning;
use crate::service::{ServiceEnvironmentVariable, ServiceTable, ServiceTableError};
use crate::shutdown::ShutdownSettings;

pub const DEFAULT_MAX_PARALLEL_STARTS: u32 = 10;
pub const DEFAULT_BOOT_SUCCESS_GRACE_SECS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Phase2BootSettings {
    pub mode: BootMode,
    pub max_parallel_starts: u32,
    pub boot_success_grace_secs: u32,
    /// Inject the compiled-in console service into the boot set (from
    /// `peios.console=1`). A per-boot command-line policy like `mode`, not a
    /// registry-configured value, so `read_effective_boot_settings` leaves it
    /// untouched.
    pub spawn_console: bool,
}

impl Default for Phase2BootSettings {
    fn default() -> Self {
        Self {
            mode: BootMode::Full,
            max_parallel_starts: DEFAULT_MAX_PARALLEL_STARTS,
            boot_success_grace_secs: DEFAULT_BOOT_SUCCESS_GRACE_SECS,
            spawn_console: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase2BootRun {
    pub settings: Phase2BootSettings,
    pub services_schema_version: u32,
    pub config_warnings: Vec<RegistryConfigWarning>,
    pub shutdown_settings: ShutdownSettings,
    pub control_security: ControlSecurityDescriptor,
    pub control_limits: ControlSocketLimits,
    pub log_config: RuntimeLogConfig,
    pub service_table: ServiceTable,
    pub global_environment: Vec<ServiceEnvironmentVariable>,
    pub eventd_log_socket_path: Option<String>,
    pub plan: Phase2BootPlan,
    pub dispatch: Phase2BootDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase2BootRunError {
    RecoveryRequired(Phase2RecoveryReason),
    Plan(Phase2BootPlanError),
    Dispatch(OperationStoreError),
    ServiceTable(ServiceTableError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase2RecoveryReason {
    InvalidMaxParallelStarts,
    RegistryRead(BoundaryError),
    Clock(BoundaryError),
}
