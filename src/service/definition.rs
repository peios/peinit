use crate::provisioning::ServiceRuntimeDirectory;

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
    /// `boot` — start during the Phase 2 boot sequence.
    Boot,
    /// `boot:settled` — start once the Phase 2 boot set has stopped moving, or
    /// a deadline expires, whichever comes first.
    ///
    /// A boot trigger, not a separate kind: the service is wanted on this boot,
    /// just not in the middle of it. It is deliberately NOT part of the boot
    /// plan — it does not consume the parallel-start budget, is not counted
    /// towards boot success, and cannot block anything — so a service that
    /// merely prefers a quiet moment cannot change what the boot means.
    ///
    /// Ordering it with `Requires` instead would say the wrong thing. A console
    /// login does not *need* the boot to be quiet in order to function; it just
    /// looks broken when its prompt is written over. That is a scheduling
    /// preference, and the trigger is where "when do I start" belongs.
    BootSettled,
    Timer {
        schedule: String,
    },
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
    /// Virtual names this service fills, as a role other services depend on
    /// without naming it.
    ///
    /// A role is not a service name, and that indirection is the point:
    /// peinit knows it needs *an* authority to mint a non-SYSTEM token, and
    /// hardcodes the socket that authority listens on, but the name of the
    /// service behind it is registry data and none of peinit's business.
    ///
    /// Virtual and real names share one namespace, exactly as they do for
    /// packages (PSPU §5.4): a dependency on `authn` is satisfied by a
    /// service literally called `authn`, or by any service providing it.
    pub provides: Vec<String>,
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
    pub runtime_directories: Vec<ServiceRuntimeDirectory>,
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
    /// When set, the launcher opens this tty and attaches the service's stdio
    /// to it (a real terminal on fds 0/1/2) instead of the daemon default
    /// (`/dev/null` stdin + captured stdout/stderr log pipes), and makes the
    /// service a session leader owning it as its controlling terminal.
    ///
    /// A path rather than a flag because which terminal is a per-service
    /// question, not a global one: a shell on `tty1` while the kernel console
    /// is a serial line is an ordinary thing to want, and `console=ttyS0`
    /// images need `/dev/console` to mean the serial line for some services and
    /// not others. Absent means daemon stdio.
    ///
    /// Attaching a tty suppresses log capture — the pipes eventd would read are
    /// closed — so output goes to the terminal and nowhere else.
    pub console_path: Option<String>,

    /// peinit defines this service itself; the registry does not and cannot.
    ///
    /// True only for registryd, which bootstraps the registry and therefore can
    /// never be an entry in it. The flag exists so a *reload* can tell "the
    /// registry no longer defines this" from "the registry never did": a
    /// snapshot omitting a compiled-in service says nothing about it, and
    /// treating that omission as a removal silently unmanages the one Critical
    /// service peinit cannot afford to lose.
    ///
    /// Provenance rather than a name check, so the rule lives beside the data
    /// and holds if the compiled-in set ever grows again. A registry service
    /// that happened to be called `registryd` would carry `false` and stay
    /// removable like any other.
    pub compiled_in: bool,
}

impl ServiceDefinition {
    pub const REGISTRYD_NAME: &'static str = "registryd";
    pub const REGISTRYD_IMAGE_PATH: &'static str = "/sbin/registryd";
    /// Hive specs handed to the Phase-1 registryd, in the `HiveName=Path` form
    /// the RSI source (loregd) parses. peinit owns this boot policy — where the
    /// machine registry lives — the way it owns `REGISTRYD_IMAGE_PATH`; loregd
    /// stays a generic daemon that dictates neither. The source creates each DB
    /// (and its parent dir) on first boot. `Machine` carries System\Services /
    /// Init / Boot (everything Phase 1 reads); `Users` is the HKU-equivalent.
    pub const REGISTRYD_ARGUMENTS: [&'static str; 2] = [
        "Machine=/var/state/loregd/Machine.hive",
        "Users=/var/state/loregd/Users.hive",
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
            provides: Vec::new(),
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
            runtime_directories: Vec::new(),
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
            console_path: None,
            compiled_in: false,
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
        // The registry cannot define the daemon that serves the registry, so a
        // reload must never read its absence from a registry snapshot as a
        // removal.
        service.compiled_in = true;
        service
    }

    /// Whether this service belongs to the Phase 2 boot plan.
    ///
    /// `boot:settled` deliberately does NOT count: those services start after
    /// the plan, so including them would put them in the parallel-start budget
    /// and in boot-success accounting, and let them block the boot they are
    /// supposed to be staying out of the way of.
    pub fn has_boot_trigger(&self) -> bool {
        self.triggers
            .iter()
            .any(|trigger| matches!(trigger, ServiceTrigger::Boot))
    }

    /// Whether this service starts once the boot set has settled.
    pub fn has_boot_settled_trigger(&self) -> bool {
        self.triggers
            .iter()
            .any(|trigger| matches!(trigger, ServiceTrigger::BootSettled))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_daemon_executes_through_a_root_level_runtime_view() {
        // registryd is the one service peinit still defines itself, so it is
        // the one image path that cannot be reviewed as registry data.
        assert!(
            ServiceDefinition::REGISTRYD_IMAGE_PATH.starts_with("/sbin/"),
            "registryd must execute through the StrataFS runtime views",
        );
    }

    #[test]
    fn compiled_in_registryd_is_a_critical_boot_daemon() {
        let registryd = ServiceDefinition::compiled_in_registryd();

        assert_eq!(registryd.name, ServiceDefinition::REGISTRYD_NAME);
        assert_eq!(
            registryd.image_path,
            ServiceDefinition::REGISTRYD_IMAGE_PATH
        );
        assert_eq!(registryd.identity, "SYSTEM");
        assert_eq!(registryd.readiness, Readiness::Notify);
        // Nothing else can be read from the registry until this is serving, so
        // a boot that cannot start it has not started.
        assert_eq!(registryd.error_control, ErrorControl::Critical);
        assert!(registryd.has_boot_trigger());
        assert!(registryd.console_path.is_none());
    }

    /// registryd is the *only* service peinit compiles in. console, authd,
    /// lpsd and login were compiled in too and are now ordinary registry
    /// services; nothing should quietly reintroduce that pattern, because a
    /// compiled-in definition cannot be inspected, overridden or disabled by
    /// an operator.
    #[test]
    fn registryd_is_the_only_compiled_in_service() {
        let registryd = ServiceDefinition::compiled_in_registryd();

        assert_eq!(registryd.name, ServiceDefinition::REGISTRYD_NAME);
        assert!(registryd.requires.is_empty());
        assert!(registryd.wants.is_empty());
        assert!(registryd.binds_to.is_empty());
    }

    #[test]
    fn simple_system_boot_does_not_attach_a_terminal() {
        let service = ServiceDefinition::simple_system_boot("svc", "/bin/svc");

        assert!(service.console_path.is_none());
    }
}
