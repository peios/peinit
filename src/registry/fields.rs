use crate::service::{
    ErrorControl, NotifyAccess, Readiness, RestartPolicy, ServiceTrigger, ServiceType,
};

use super::{RegistryValueType, ServiceRegistryDecodeError};

/// What a field holds, as a person states it, and so which registry type it
/// is stored as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Text, stored as `REG_SZ`.
    Text,
    /// An ordered list of text, stored as `REG_MULTI_SZ`.
    List,
    /// A count or an amount, in `unit` where it has one, stored as
    /// `REG_DWORD`.
    Number { unit: Option<&'static str> },
    /// Yes (1) or no (0), stored as `REG_DWORD`.
    YesNo,
    /// One of these values, each by its name, stored as `REG_DWORD`.
    Choice(&'static [(u32, &'static str)]),
    /// Bytes, stored as `REG_BINARY`.
    Binary,
}

impl FieldKind {
    /// The registry type the field is stored as.
    pub fn value_type(self) -> RegistryValueType {
        match self {
            Self::Text => RegistryValueType::Sz,
            Self::List => RegistryValueType::MultiSz,
            Self::Number { .. } | Self::YesNo | Self::Choice(_) => RegistryValueType::Dword,
            Self::Binary => RegistryValueType::Binary,
        }
    }
}

/// What a field is about, as the registry key reference groups them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldGroup {
    /// What the service is called and what it does.
    About,
    /// What runs, where, and with what around it.
    Execution,
    /// Simple or Oneshot, and when it counts as started.
    TypeAndReadiness,
    /// What starts it, and what it checks first.
    Activation,
    /// Who it runs as.
    Identity,
    /// What it needs, wants, and stands for.
    Dependencies,
    /// Restarting it, and watching that it is well.
    Supervision,
    /// What runs around starting, reloading and stopping it, and how long
    /// each may take.
    TransitionPhases,
    /// Notification, the fd store, and its control descriptor.
    Other,
}

impl FieldGroup {
    /// Every group, in the order to show them.
    pub const ALL: [FieldGroup; 9] = [
        Self::About,
        Self::Execution,
        Self::TypeAndReadiness,
        Self::Activation,
        Self::Identity,
        Self::Dependencies,
        Self::Supervision,
        Self::TransitionPhases,
        Self::Other,
    ];

    /// What it is called, as a heading.
    pub fn title(self) -> &'static str {
        match self {
            Self::About => "About",
            Self::Execution => "Execution",
            Self::TypeAndReadiness => "Type and readiness",
            Self::Activation => "Activation",
            Self::Identity => "Identity",
            Self::Dependencies => "Dependencies",
            Self::Supervision => "Supervision and health",
            Self::TransitionPhases => "Starting, reloading and stopping",
            Self::Other => "Other",
        }
    }
}

/// When a changed field takes effect on a service that is running. On one
/// that is not, everything takes effect when it next starts, and its
/// triggers as soon as peinit has read the change (TRM §3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakesEffect {
    /// Held to the running definition: at the next restart.
    Restart,
    /// Applied when it next starts, or at an explicit graph reload: for a
    /// running service, at the next restart.
    NextStart,
    /// Reloaded at runtime: the next time peinit acts on it.
    Runtime,
}

/// One field of a service definition: its value's name, and what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldInfo {
    pub name: &'static str,
    pub kind: FieldKind,
    pub group: FieldGroup,
    pub takes_effect: TakesEffect,
    /// What applies when the value is absent, in words, or `None` where
    /// absence means nothing is set.
    pub default: Option<&'static str>,
}

const SECONDS: FieldKind = FieldKind::Number { unit: Some("seconds") };
const COUNT: FieldKind = FieldKind::Number { unit: None };

macro_rules! service_fields {
    ($(($name:literal, $variant:ident, $kind:expr, $group:ident, $effect:ident, $default:expr)),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub(super) enum Field {
            $($variant),+
        }

        const SERVICE_FIELDS: &[(&str, Field)] = &[
            $(($name, Field::$variant)),+
        ];

        /// Every field of a service definition. `group` says where each
        /// belongs; the order here is no one's to rely on.
        pub const SERVICE_FIELD_INFO: &[FieldInfo] = &[
            $(FieldInfo {
                name: $name,
                kind: $kind,
                group: FieldGroup::$group,
                takes_effect: TakesEffect::$effect,
                default: $default,
            }),+
        ];

        impl Field {
            pub(super) fn name(self) -> &'static str {
                match self {
                    $(Field::$variant => $name),+
                }
            }
        }
    };
}

service_fields! {
    ("ImagePath", ImagePath, FieldKind::Text, Execution, Restart, None),
    ("Arguments", Arguments, FieldKind::List, Execution, Runtime, None),
    ("Type", Type, FieldKind::Choice(&[(0, "Simple"), (1, "Oneshot")]), TypeAndReadiness, Restart, Some("Simple")),
    ("Triggers", Triggers, FieldKind::List, Activation, Restart, None),
    ("Disabled", Disabled, FieldKind::YesNo, Activation, Restart, Some("no")),
    ("SafeMode", SafeMode, FieldKind::YesNo, Activation, Runtime, Some("no")),
    ("Identity", Identity, FieldKind::Text, Identity, Restart, Some("LocalService")),
    ("RequiredPrivileges", RequiredPrivileges, FieldKind::List, Identity, Restart, None),
    ("Requires", Requires, FieldKind::List, Dependencies, NextStart, None),
    ("Wants", Wants, FieldKind::List, Dependencies, NextStart, None),
    ("BindsTo", BindsTo, FieldKind::List, Dependencies, NextStart, None),
    ("Conflicts", Conflicts, FieldKind::List, Dependencies, NextStart, None),
    ("Provides", Provides, FieldKind::List, Dependencies, Runtime, None),
    ("OnFailure", OnFailure, FieldKind::Text, Dependencies, NextStart, None),
    ("Readiness", Readiness, FieldKind::Choice(&[(0, "Notify"), (1, "Alive")]), TypeAndReadiness, Runtime, Some("Notify")),
    ("NotifyAccess", NotifyAccess, FieldKind::Choice(&[(0, "Main")]), Other, Runtime, Some("Main")),
    ("RemainAfterExit", RemainAfterExit, FieldKind::YesNo, TypeAndReadiness, Restart, Some("no")),
    ("SuccessExitCodes", SuccessExitCodes, FieldKind::List, TypeAndReadiness, Runtime, None),
    ("ExecStartPre", ExecStartPre, FieldKind::List, TransitionPhases, Runtime, None),
    ("ExecStartPost", ExecStartPost, FieldKind::List, TransitionPhases, Runtime, None),
    ("HookIdentity", HookIdentity, FieldKind::Text, Identity, Runtime, Some("the service's Identity")),
    ("ExecReload", ExecReload, FieldKind::Text, TransitionPhases, Runtime, Some("signal:HUP")),
    ("PreStartCheckTimeout", PreStartCheckTimeout, SECONDS, Activation, Runtime, Some("5")),
    ("StartTimeout", StartTimeout, SECONDS, TransitionPhases, Runtime, Some("30")),
    ("StopTimeout", StopTimeout, SECONDS, TransitionPhases, Runtime, Some("10")),
    ("WatchdogTimeout", WatchdogTimeout, SECONDS, Supervision, Runtime, Some("0, none")),
    ("HealthCheck", HealthCheck, FieldKind::Text, Supervision, Runtime, None),
    ("HealthCheckInterval", HealthCheckInterval, SECONDS, Supervision, Runtime, Some("30")),
    ("HealthCheckTimeout", HealthCheckTimeout, SECONDS, Supervision, Runtime, Some("5")),
    ("HealthCheckRetries", HealthCheckRetries, COUNT, Supervision, Runtime, Some("3")),
    ("FdStoreMax", FdStoreMax, COUNT, Other, Runtime, Some("0, none")),
    ("Environment", Environment, FieldKind::List, Execution, Runtime, None),
    ("WorkingDirectory", WorkingDirectory, FieldKind::Text, Execution, Runtime, Some("/")),
    ("RuntimeDirectories", RuntimeDirectories, FieldKind::List, Execution, Runtime, None),
    ("LimitNOFILE", LimitNoFile, COUNT, Execution, Runtime, None),
    ("LimitCORE", LimitCore, FieldKind::Number { unit: Some("bytes") }, Execution, Runtime, None),
    ("Conditions", Conditions, FieldKind::List, Activation, NextStart, None),
    ("Asserts", Asserts, FieldKind::List, Activation, NextStart, None),
    ("DisplayName", DisplayName, FieldKind::Text, About, Runtime, None),
    ("Description", Description, FieldKind::Text, About, Runtime, None),
    ("RestartPolicy", RestartPolicy, FieldKind::Choice(&[(0, "Never"), (1, "OnFailure"), (2, "Always")]), Supervision, Runtime, Some("OnFailure")),
    ("RestartMaxRetries", RestartMaxRetries, COUNT, Supervision, Runtime, Some("5")),
    ("RestartWindow", RestartWindow, SECONDS, Supervision, Runtime, Some("120")),
    ("RestartDelay", RestartDelay, SECONDS, Supervision, Runtime, Some("1")),
    ("ErrorControl", ErrorControl, FieldKind::Choice(&[(0, "Normal"), (1, "Critical")]), Supervision, Restart, Some("Normal")),
    ("ServiceSecurity", ServiceSecurity, FieldKind::Binary, Other, Runtime, Some("the Services key's, or the built-in default")),
    ("TimerPersistent", TimerPersistent, FieldKind::YesNo, Activation, Runtime, Some("yes")),
    ("TTYPath", TtyPath, FieldKind::Text, Execution, Runtime, None),
    ("TTYPrecedence", TtyPrecedence, COUNT, Execution, Runtime, Some("0")),
    ("TimerJitter", TimerJitter, SECONDS, Activation, Runtime, Some("0")),
}

pub(super) fn field_from_name(name: &str) -> Option<Field> {
    SERVICE_FIELDS
        .iter()
        .find_map(|(candidate, field)| candidate.eq_ignore_ascii_case(name).then_some(*field))
}

/// The field called `name`, in any case.
pub fn service_field(name: &str) -> Option<&'static FieldInfo> {
    SERVICE_FIELD_INFO.iter().find(|field| field.name.eq_ignore_ascii_case(name))
}

pub(super) fn parse_timer_persistent(value: u32) -> Result<bool, ServiceRegistryDecodeError> {
    parse_bool("TimerPersistent", value)
}

pub(super) fn parse_notify_access(value: u32) -> Result<NotifyAccess, ServiceRegistryDecodeError> {
    match value {
        0 => Ok(NotifyAccess::Main),
        _ => Err(ServiceRegistryDecodeError::UnknownDword {
            field: "NotifyAccess",
            value,
        }),
    }
}

pub(super) fn parse_error_control(value: u32) -> Result<ErrorControl, ServiceRegistryDecodeError> {
    match value {
        0 => Ok(ErrorControl::Normal),
        1 => Ok(ErrorControl::Critical),
        _ => Err(ServiceRegistryDecodeError::UnknownDword {
            field: "ErrorControl",
            value,
        }),
    }
}

/// Boot sub-triggers, as a closed set.
///
/// `boot` takes no argument, so `boot:<anything>` was rejected outright — the
/// same arity rule that rejects a bare `timer`. Opening the namespace keeps
/// that rule rather than dropping it: a listed sub-type is accepted and
/// everything else is still an error, so `boot:setled` is caught instead of
/// being silently tolerated as an unknown trigger kind.
const BOOT_SUB_TRIGGERS: &[(&str, ServiceTrigger)] = &[("settled", ServiceTrigger::BootSettled)];

/// Terminal sub-triggers, as a closed set, on the same rule as `boot:`.
///
/// `tty` alone is not a trigger — there is no event called "a terminal" — so
/// the bare word is rejected like a bare `timer`, and an unlisted sub-type is
/// an error rather than an unknown trigger kind.
const TTY_SUB_TRIGGERS: &[(&str, ServiceTrigger)] = &[("released", ServiceTrigger::TtyReleased)];

pub(super) fn classify_trigger(
    value: String,
) -> Result<ServiceTrigger, ServiceRegistryDecodeError> {
    if value == "boot" {
        Ok(ServiceTrigger::Boot)
    } else if let Some(sub) = value.strip_prefix("boot:") {
        BOOT_SUB_TRIGGERS
            .iter()
            .find(|(name, _)| *name == sub)
            .map(|(_, trigger)| trigger.clone())
            .ok_or(ServiceRegistryDecodeError::InvalidTrigger { value })
    } else if value == "tty" {
        Err(ServiceRegistryDecodeError::InvalidTrigger { value })
    } else if let Some(sub) = value.strip_prefix("tty:") {
        TTY_SUB_TRIGGERS
            .iter()
            .find(|(name, _)| *name == sub)
            .map(|(_, trigger)| trigger.clone())
            .ok_or(ServiceRegistryDecodeError::InvalidTrigger { value })
    } else if value == "timer"
        || value == "timer:"
        || value.is_empty()
        || value.starts_with(':')
        || value.ends_with(':')
    {
        Err(ServiceRegistryDecodeError::InvalidTrigger { value })
    } else if let Some(schedule) = value.strip_prefix("timer:") {
        Ok(ServiceTrigger::Timer {
            schedule: schedule.to_string(),
        })
    } else {
        Ok(ServiceTrigger::Other(value))
    }
}

pub(super) fn parse_service_type(value: u32) -> Result<ServiceType, ServiceRegistryDecodeError> {
    match value {
        0 => Ok(ServiceType::Simple),
        1 => Ok(ServiceType::Oneshot),
        _ => Err(ServiceRegistryDecodeError::UnknownDword {
            field: "Type",
            value,
        }),
    }
}

pub(super) fn parse_restart_policy(
    value: u32,
) -> Result<RestartPolicy, ServiceRegistryDecodeError> {
    match value {
        0 => Ok(RestartPolicy::Never),
        1 => Ok(RestartPolicy::OnFailure),
        2 => Ok(RestartPolicy::Always),
        _ => Err(ServiceRegistryDecodeError::UnknownDword {
            field: "RestartPolicy",
            value,
        }),
    }
}

pub(super) fn parse_readiness(value: u32) -> Result<Readiness, ServiceRegistryDecodeError> {
    match value {
        0 => Ok(Readiness::Notify),
        1 => Ok(Readiness::Alive),
        _ => Err(ServiceRegistryDecodeError::UnknownDword {
            field: "Readiness",
            value,
        }),
    }
}

pub(super) fn parse_success_exit_codes(
    values: Vec<String>,
) -> Result<Vec<i32>, ServiceRegistryDecodeError> {
    let mut codes = Vec::new();
    for value in values {
        let Ok(code) = value.parse::<i32>() else {
            return Err(ServiceRegistryDecodeError::InvalidSuccessExitCode { value });
        };
        if !(0..=255).contains(&code) {
            return Err(ServiceRegistryDecodeError::InvalidSuccessExitCode { value });
        }
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    Ok(codes)
}

pub(super) fn parse_bool(
    field: &'static str,
    value: u32,
) -> Result<bool, ServiceRegistryDecodeError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ServiceRegistryDecodeError::UnknownDword { field, value }),
    }
}
