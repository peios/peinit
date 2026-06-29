mod model;
mod orchestrator;

#[cfg(all(feature = "peios-boundary", feature = "peios-registry"))]
mod linux;

#[cfg(test)]
mod tests;

#[cfg(all(feature = "peios-boundary", feature = "peios-registry"))]
pub use linux::{LinuxInitError, run_linux_peinit};
pub use model::{
    DEFAULT_BOOT_ATTEMPT_THRESHOLD, InitConfig, InitFatalError, InitPlatform, InitRecoveryReason,
    InitRunError, InitRunResult, InitRuntime, KernelCommandLine, Phase1Infrastructure,
    Phase1InfrastructureWarning, Phase1JfsDevice,
};
pub use orchestrator::run_init;
