#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    Simple,
    Oneshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Alive,
    Notify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    OnFailure,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorControl {
    Normal,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyAccess {
    Main,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceTrigger {
    Boot,
    Timer { schedule: String },
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceCheckKind {
    Path,
    File,
    Directory,
    Registry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCheck {
    pub kind: ServiceCheckKind,
    pub argument: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ServiceSecurityDescriptor {
    #[default]
    Default,
    RegistryBinary(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceEnvironmentVariable {
    pub name: String,
    pub value: String,
}

pub fn is_valid_service_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDefinition {
    pub name: String,
    pub image_path: String,
    pub arguments: Vec<String>,
    pub service_type: ServiceType,
    pub triggers: Vec<ServiceTrigger>,
    pub disabled: bool,
    pub safe_mode: bool,
    pub identity: String,
    pub required_privileges: Vec<String>,
    pub requires: Vec<String>,
    pub wants: Vec<String>,
    pub binds_to: Vec<String>,
    pub conflicts: Vec<String>,
    pub on_failure: Option<String>,
    pub readiness: Readiness,
    pub notify_access: NotifyAccess,
    pub remain_after_exit: bool,
    pub success_exit_codes: Vec<i32>,
    pub exec_start_pre: Vec<String>,
    pub exec_start_post: Vec<String>,
    pub hook_identity: Option<String>,
    pub exec_reload: Option<String>,
    pub pre_start_check_timeout_secs: u64,
    pub start_timeout_secs: u64,
    pub stop_timeout_secs: u64,
    pub watchdog_timeout_secs: u64,
    pub health_check: Option<String>,
    pub health_check_interval_secs: u64,
    pub health_check_timeout_secs: u64,
    pub health_check_retries: u32,
    pub fd_store_max: u32,
    pub environment: Vec<ServiceEnvironmentVariable>,
    pub working_directory: String,
    pub limit_nofile: Option<u64>,
    pub limit_core: Option<u64>,
    pub conditions: Vec<ServiceCheck>,
    pub asserts: Vec<ServiceCheck>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub restart_policy: RestartPolicy,
    pub restart_max_retries: u32,
    pub restart_window_secs: u64,
    pub restart_delay_secs: u64,
    pub error_control: ErrorControl,
    pub service_security: ServiceSecurityDescriptor,
    pub timer_persistent: bool,
    pub timer_jitter_secs: u64,
    /// When set, the launcher attaches this service's stdio to `/dev/console`
    /// (a real tty on fds 0/1/2) instead of the daemon default (`/dev/null`
    /// stdin + captured stdout/stderr log pipes). Reserved for the compiled-in
    /// console service; never set from a registry definition.
    pub attach_console: bool,
}

impl ServiceDefinition {
    pub const REGISTRYD_NAME: &'static str = "registryd";
    pub const REGISTRYD_IMAGE_PATH: &'static str = "/usr/sbin/registryd";
    /// The compiled-in console service: a SYSTEM shell on `/dev/console`,
    /// injected into the Phase 2 boot set only when `peios.console=1` is on the
    /// kernel command line. Not a registry entry — like registryd, peinit owns
    /// it directly.
    pub const CONSOLE_NAME: &'static str = "console";
    pub const CONSOLE_IMAGE_PATH: &'static str = "/usr/bin/sh";
    /// Hive specs handed to the Phase-1 registryd, in the `HiveName=Path` form
    /// the RSI source (loregd) parses. peinit owns this boot policy — where the
    /// machine registry lives — the way it owns `REGISTRYD_IMAGE_PATH`; loregd
    /// stays a generic daemon that dictates neither. The source creates each DB
    /// (and its parent dir) on first boot. `Machine` carries System\Services /
    /// Init / Boot (everything Phase 1 reads); `Users` is the HKU-equivalent.
    pub const REGISTRYD_ARGUMENTS: [&'static str; 2] = [
        "Machine=/var/lib/loregd/Machine.hive",
        "Users=/var/lib/loregd/Users.hive",
    ];
    pub const DEFAULT_RESTART_POLICY: RestartPolicy = RestartPolicy::OnFailure;
    pub const DEFAULT_RESTART_MAX_RETRIES: u32 = 5;
    pub const DEFAULT_RESTART_WINDOW_SECS: u64 = 120;
    pub const DEFAULT_RESTART_DELAY_SECS: u64 = 1;
    pub const DEFAULT_ERROR_CONTROL: ErrorControl = ErrorControl::Normal;
    pub const DEFAULT_PRE_START_CHECK_TIMEOUT_SECS: u64 = 5;
    pub const DEFAULT_START_TIMEOUT_SECS: u64 = 30;
    pub const DEFAULT_STOP_TIMEOUT_SECS: u64 = 10;
    pub const DEFAULT_WATCHDOG_TIMEOUT_SECS: u64 = 0;
    pub const DEFAULT_HEALTH_CHECK_INTERVAL_SECS: u64 = 30;
    pub const DEFAULT_HEALTH_CHECK_TIMEOUT_SECS: u64 = 5;
    pub const DEFAULT_HEALTH_CHECK_RETRIES: u32 = 3;
    pub const DEFAULT_FD_STORE_MAX: u32 = 0;
    pub const DEFAULT_WORKING_DIRECTORY: &'static str = "/";
    pub const DEFAULT_TIMER_PERSISTENT: bool = true;
    pub const DEFAULT_TIMER_JITTER_SECS: u64 = 0;

    pub fn simple_system_boot(name: &str, image_path: &str) -> Self {
        Self {
            name: name.to_string(),
            image_path: image_path.to_string(),
            arguments: Vec::new(),
            service_type: ServiceType::Simple,
            triggers: vec![ServiceTrigger::Boot],
            disabled: false,
            safe_mode: false,
            identity: "SYSTEM".to_string(),
            required_privileges: Vec::new(),
            requires: Vec::new(),
            wants: Vec::new(),
            binds_to: Vec::new(),
            conflicts: Vec::new(),
            on_failure: None,
            readiness: Readiness::Notify,
            notify_access: NotifyAccess::Main,
            remain_after_exit: false,
            success_exit_codes: Vec::new(),
            exec_start_pre: Vec::new(),
            exec_start_post: Vec::new(),
            hook_identity: None,
            exec_reload: None,
            pre_start_check_timeout_secs: Self::DEFAULT_PRE_START_CHECK_TIMEOUT_SECS,
            start_timeout_secs: Self::DEFAULT_START_TIMEOUT_SECS,
            stop_timeout_secs: Self::DEFAULT_STOP_TIMEOUT_SECS,
            watchdog_timeout_secs: Self::DEFAULT_WATCHDOG_TIMEOUT_SECS,
            health_check: None,
            health_check_interval_secs: Self::DEFAULT_HEALTH_CHECK_INTERVAL_SECS,
            health_check_timeout_secs: Self::DEFAULT_HEALTH_CHECK_TIMEOUT_SECS,
            health_check_retries: Self::DEFAULT_HEALTH_CHECK_RETRIES,
            fd_store_max: Self::DEFAULT_FD_STORE_MAX,
            environment: Vec::new(),
            working_directory: Self::DEFAULT_WORKING_DIRECTORY.to_string(),
            limit_nofile: None,
            limit_core: None,
            conditions: Vec::new(),
            asserts: Vec::new(),
            display_name: None,
            description: None,
            restart_policy: Self::DEFAULT_RESTART_POLICY,
            restart_max_retries: Self::DEFAULT_RESTART_MAX_RETRIES,
            restart_window_secs: Self::DEFAULT_RESTART_WINDOW_SECS,
            restart_delay_secs: Self::DEFAULT_RESTART_DELAY_SECS,
            error_control: Self::DEFAULT_ERROR_CONTROL,
            service_security: ServiceSecurityDescriptor::Default,
            timer_persistent: Self::DEFAULT_TIMER_PERSISTENT,
            timer_jitter_secs: Self::DEFAULT_TIMER_JITTER_SECS,
            attach_console: false,
        }
    }

    pub fn compiled_in_registryd() -> Self {
        let mut service =
            Self::simple_system_boot(Self::REGISTRYD_NAME, Self::REGISTRYD_IMAGE_PATH);
        service.arguments = Self::REGISTRYD_ARGUMENTS
            .iter()
            .map(|arg| arg.to_string())
            .collect();
        service.error_control = ErrorControl::Critical;
        service
    }

    /// The compiled-in console service: a SYSTEM shell on `/dev/console`.
    ///
    /// Unlike registryd it is *not* a notify-readiness daemon — a shell never
    /// sends `READY=1`, so readiness is `Alive` (active once exec'd). It is
    /// respawned forever (`RestartPolicy::Always`): exiting the shell yields a
    /// fresh prompt rather than a dead console. `attach_console` routes its
    /// stdio to the console tty in the launcher. Environment mirrors the
    /// recovery console's shell so the prompt behaves the same.
    pub fn compiled_in_console() -> Self {
        let mut service = Self::simple_system_boot(Self::CONSOLE_NAME, Self::CONSOLE_IMAGE_PATH);
        service.readiness = Readiness::Alive;
        service.restart_policy = RestartPolicy::Always;
        service.attach_console = true;
        service.environment = vec![
            ServiceEnvironmentVariable {
                name: "PATH".to_string(),
                value: "/usr/sbin:/usr/bin:/sbin:/bin".to_string(),
            },
            ServiceEnvironmentVariable {
                name: "TERM".to_string(),
                value: "linux".to_string(),
            },
            ServiceEnvironmentVariable {
                name: "HOME".to_string(),
                value: "/root".to_string(),
            },
        ];
        service
    }

    pub fn has_boot_trigger(&self) -> bool {
        self.triggers
            .iter()
            .any(|trigger| matches!(trigger, ServiceTrigger::Boot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_in_console_attaches_a_respawning_alive_system_shell() {
        let console = ServiceDefinition::compiled_in_console();

        assert_eq!(console.name, ServiceDefinition::CONSOLE_NAME);
        assert_eq!(console.image_path, ServiceDefinition::CONSOLE_IMAGE_PATH);
        assert_eq!(console.identity, "SYSTEM");
        assert!(console.attach_console);
        // A shell never sends READY=1, so it must be Alive (active once exec'd)
        // and respawned forever so exiting the shell yields a fresh prompt.
        assert_eq!(console.readiness, Readiness::Alive);
        assert_eq!(console.restart_policy, RestartPolicy::Always);
        assert!(console.has_boot_trigger());
    }

    #[test]
    fn simple_system_boot_does_not_attach_a_console() {
        let service = ServiceDefinition::simple_system_boot("svc", "/usr/bin/svc");

        assert!(!service.attach_console);
    }
}
