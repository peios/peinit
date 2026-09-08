mod eventd;
#[cfg(feature = "peios-boundary")]
mod linux_boot_attempts;
mod linux_child;
mod linux_clock;
#[cfg(feature = "peios-boundary")]
mod linux_console;
mod linux_epoll;
mod linux_io;
#[cfg(feature = "peios-boundary")]
mod linux_kmes;
mod linux_machine_id;
mod linux_power;
#[cfg(feature = "peios-boundary")]
mod linux_pre_start_check;
#[cfg(feature = "peios-boundary")]
mod linux_privileges;
mod linux_provisioning;
mod linux_random_seed;
mod linux_signal;
mod linux_timer;
mod model;

#[cfg(feature = "peios-boundary")]
mod linux_jobs;
#[cfg(feature = "peios-boundary")]
mod linux_launch;
#[cfg(feature = "peios-boundary")]
mod linux_process;

pub use eventd::{EventdLogSink, EventdSendOutcome, LinuxEventdLogSink, send_eventd_log_record};
#[cfg(feature = "peios-boundary")]
pub use linux_boot_attempts::LinuxBootAttemptCounter;
pub use linux_child::{
    LinuxChildReapError, LinuxChildReapSyscallApi, LinuxChildReaper, LinuxWaitPid,
    drain_linux_child_reaps, normalize_linux_wait_status, wait_linux_child,
};
pub use linux_clock::{
    LinuxClockError, LinuxClockSyscallApi, LinuxMonotonicClock, linux_monotonic_ns,
};
#[cfg(feature = "peios-boundary")]
pub use linux_console::{
    CONSOLE_PATH, LinuxConsoleSink, open_linux_console_fd, write_linux_console_message,
};
pub use linux_epoll::{
    LinuxEpoll, LinuxEpollCreateError, LinuxEpollEvent, LinuxEpollRegisterError,
    LinuxEpollSyscallApi, LinuxEpollUnregisterError, LinuxEpollWaitError, create_linux_epoll,
    register_linux_epoll_read, unregister_linux_epoll, wait_linux_epoll,
};
#[cfg(feature = "peios-boundary")]
pub(crate) use linux_io::{read_fd_to_string, set_cloexec, write_all_fd};
#[cfg(feature = "peios-boundary")]
pub use linux_jobs::LinuxJobIdentityProvider;
#[cfg(feature = "peios-boundary")]
pub use linux_kmes::LinuxKmesEventSink;
#[cfg(feature = "peios-boundary")]
pub use linux_launch::{LinuxProcessLauncher, LinuxSystemTokenProvider};
pub use linux_machine_id::{
    DEFAULT_MACHINE_ID_PATH, LinuxMachineIdError, LinuxMachineIdStatus, ensure_linux_machine_id,
};
#[cfg(feature = "peios-boundary")]
pub use linux_power::LinuxPowerButtonDevices;
pub use linux_power::{LinuxPowerButtonRead, LinuxPowerButtonReadError};
#[cfg(feature = "peios-boundary")]
pub use linux_pre_start_check::LinuxFilesystemCheckHelper;
#[cfg(feature = "peios-boundary")]
pub use linux_privileges::verify_peinit_privileges;
#[cfg(feature = "peios-boundary")]
pub use linux_process::LinuxProcessController;
#[cfg(all(feature = "peios-boundary", feature = "peios-registry"))]
pub(crate) use linux_provisioning::{ensure_runtime_directory, set_path_security};
pub use linux_provisioning::{
    provision_linux_boot_paths, provision_linux_service_runtime_directories,
};
pub use linux_random_seed::{
    DEFAULT_RANDOM_SEED_PATH, LinuxRandomSeedError, LinuxRandomSeedRestoreStatus,
    restore_linux_random_seed, save_linux_random_seed,
};
pub use linux_signal::{
    LinuxPid1SignalFd, LinuxSignalFdRead, LinuxSignalFdReadError, LinuxSignalMask,
    Pid1SignalFdRegisteredSetup, Pid1SignalFdRegisteredSetupError, Pid1SignalFdSetup,
    Pid1SignalFdSetupError, Pid1SignalFdSyscalls, setup_pid1_signalfd,
    setup_pid1_signalfd_registered,
};
pub use linux_timer::{
    LinuxTimerFd, LinuxTimerFdArmError, LinuxTimerFdCreateError, LinuxTimerFdRead,
    LinuxTimerFdReadError, LinuxTimerFdSyscallApi, LinuxTimerSpec, create_linux_monotonic_timerfd,
    linux_timer_spec_from_deadline_ns, read_linux_timerfd, set_linux_timerfd_absolute,
};
pub use model::*;
