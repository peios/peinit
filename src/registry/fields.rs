use crate::service::{
    ErrorControl, NotifyAccess, Readiness, RestartPolicy, ServiceTrigger, ServiceType,
};

use super::ServiceRegistryDecodeError;

macro_rules! service_fields {
    ($(($name:literal, $variant:ident)),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub(super) enum Field {
            $($variant),+
        }

        const SERVICE_FIELDS: &[(&str, Field)] = &[
            $(($name, Field::$variant)),+
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
    ("ImagePath", ImagePath),
    ("Arguments", Arguments),
    ("Type", Type),
    ("Triggers", Triggers),
    ("Disabled", Disabled),
    ("SafeMode", SafeMode),
    ("Identity", Identity),
    ("RequiredPrivileges", RequiredPrivileges),
    ("Requires", Requires),
    ("Wants", Wants),
    ("BindsTo", BindsTo),
    ("Conflicts", Conflicts),
    ("OnFailure", OnFailure),
    ("Readiness", Readiness),
    ("NotifyAccess", NotifyAccess),
    ("RemainAfterExit", RemainAfterExit),
    ("SuccessExitCodes", SuccessExitCodes),
    ("ExecStartPre", ExecStartPre),
    ("ExecStartPost", ExecStartPost),
    ("HookIdentity", HookIdentity),
    ("ExecReload", ExecReload),
    ("PreStartCheckTimeout", PreStartCheckTimeout),
    ("StartTimeout", StartTimeout),
    ("StopTimeout", StopTimeout),
    ("WatchdogTimeout", WatchdogTimeout),
    ("HealthCheck", HealthCheck),
    ("HealthCheckInterval", HealthCheckInterval),
    ("HealthCheckTimeout", HealthCheckTimeout),
    ("HealthCheckRetries", HealthCheckRetries),
    ("FdStoreMax", FdStoreMax),
    ("Environment", Environment),
    ("WorkingDirectory", WorkingDirectory),
    ("RuntimeDirectories", RuntimeDirectories),
    ("LimitNOFILE", LimitNoFile),
    ("LimitCORE", LimitCore),
    ("Conditions", Conditions),
    ("Asserts", Asserts),
    ("DisplayName", DisplayName),
    ("Description", Description),
    ("RestartPolicy", RestartPolicy),
    ("RestartMaxRetries", RestartMaxRetries),
    ("RestartWindow", RestartWindow),
    ("RestartDelay", RestartDelay),
    ("ErrorControl", ErrorControl),
    ("ServiceSecurity", ServiceSecurity),
    ("TimerPersistent", TimerPersistent),
    ("TimerJitter", TimerJitter),
}

pub(super) fn field_from_name(name: &str) -> Option<Field> {
    SERVICE_FIELDS
        .iter()
        .find_map(|(candidate, field)| candidate.eq_ignore_ascii_case(name).then_some(*field))
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

pub(super) fn classify_trigger(
    value: String,
) -> Result<ServiceTrigger, ServiceRegistryDecodeError> {
    if value == "boot" {
        Ok(ServiceTrigger::Boot)
    } else if value.starts_with("boot:")
        || value == "timer"
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
