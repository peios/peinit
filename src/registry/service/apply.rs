use crate::service::ServiceSecurityDescriptor;

use super::super::fields::{
    Field, classify_trigger, parse_bool, parse_error_control, parse_notify_access, parse_readiness,
    parse_restart_policy, parse_service_type, parse_success_exit_codes, parse_timer_persistent,
};
use super::super::value::{
    RawRegistryValue, ServiceRegistryDecodeError, decode_binary_field, decode_dword_field,
    decode_multi_sz_field, decode_sz_field,
};
use super::builder::DefinitionBuilder;
use super::parse::{
    parse_absolute_path_field, parse_environment_variables, parse_executable_command_field,
    parse_executable_command_list, parse_identity_field, parse_non_empty_list,
    parse_optional_string, parse_service_checks, parse_service_reference_field,
    parse_service_reference_list, validate_exec_reload,
};

pub(super) fn apply_service_field(
    builder: &mut DefinitionBuilder,
    field: Field,
    value: &RawRegistryValue,
) -> Result<(), ServiceRegistryDecodeError> {
    match field {
        Field::ImagePath => {
            builder.image_path = Some(parse_absolute_path_field(value, Field::ImagePath)?);
        }
        Field::Arguments => {
            builder.arguments = decode_multi_sz_field(value, "Arguments")?;
        }
        Field::Type => {
            builder.service_type = parse_service_type(decode_dword_field(value, "Type")?)?;
        }
        Field::Triggers => {
            builder.triggers = decode_multi_sz_field(value, "Triggers")?
                .into_iter()
                .map(classify_trigger)
                .collect::<Result<Vec<_>, _>>()?;
        }
        Field::Disabled => {
            builder.disabled = parse_bool("Disabled", decode_dword_field(value, "Disabled")?)?;
        }
        Field::SafeMode => {
            builder.safe_mode = parse_bool("SafeMode", decode_dword_field(value, "SafeMode")?)?;
        }
        Field::Identity => {
            if let Some(parsed) = parse_identity_field(value, Field::Identity)? {
                builder.identity = parsed;
            }
        }
        Field::RequiredPrivileges => {
            builder.required_privileges = parse_non_empty_list(value, Field::RequiredPrivileges)?;
        }
        Field::Requires => {
            builder.requires = parse_service_reference_list(value, Field::Requires)?;
        }
        Field::Wants => {
            builder.wants = parse_service_reference_list(value, Field::Wants)?;
        }
        Field::BindsTo => {
            builder.binds_to = parse_service_reference_list(value, Field::BindsTo)?;
        }
        Field::Conflicts => {
            builder.conflicts = parse_service_reference_list(value, Field::Conflicts)?;
        }
        Field::OnFailure => {
            builder.on_failure = Some(parse_service_reference_field(value, Field::OnFailure)?);
        }
        Field::Readiness => {
            builder.readiness = parse_readiness(decode_dword_field(value, "Readiness")?)?;
        }
        Field::NotifyAccess => {
            builder.notify_access =
                parse_notify_access(decode_dword_field(value, "NotifyAccess")?)?;
        }
        Field::RemainAfterExit => {
            builder.remain_after_exit = parse_bool(
                "RemainAfterExit",
                decode_dword_field(value, "RemainAfterExit")?,
            )?;
        }
        Field::SuccessExitCodes => {
            builder.success_exit_codes =
                parse_success_exit_codes(decode_multi_sz_field(value, "SuccessExitCodes")?)?;
        }
        Field::ExecStartPre => {
            builder.exec_start_pre = parse_executable_command_list(value, Field::ExecStartPre)?;
        }
        Field::ExecStartPost => {
            builder.exec_start_post = parse_executable_command_list(value, Field::ExecStartPost)?;
        }
        Field::HookIdentity => {
            builder.hook_identity = parse_identity_field(value, Field::HookIdentity)?;
        }
        Field::ExecReload => {
            let parsed = decode_sz_field(value, "ExecReload")?;
            validate_exec_reload(&parsed)?;
            builder.exec_reload = Some(parsed);
        }
        Field::PreStartCheckTimeout => {
            builder.pre_start_check_timeout_secs =
                u64::from(decode_dword_field(value, "PreStartCheckTimeout")?);
        }
        Field::StartTimeout => {
            builder.start_timeout_secs = u64::from(decode_dword_field(value, "StartTimeout")?);
        }
        Field::StopTimeout => {
            builder.stop_timeout_secs = u64::from(decode_dword_field(value, "StopTimeout")?);
        }
        Field::WatchdogTimeout => {
            builder.watchdog_timeout_secs =
                u64::from(decode_dword_field(value, "WatchdogTimeout")?);
        }
        Field::HealthCheck => {
            let parsed = parse_executable_command_field(value, Field::HealthCheck)?;
            builder.health_check = Some(parsed);
        }
        Field::HealthCheckInterval => {
            builder.health_check_interval_secs =
                u64::from(decode_dword_field(value, "HealthCheckInterval")?);
        }
        Field::HealthCheckTimeout => {
            builder.health_check_timeout_secs =
                u64::from(decode_dword_field(value, "HealthCheckTimeout")?);
        }
        Field::HealthCheckRetries => {
            builder.health_check_retries = decode_dword_field(value, "HealthCheckRetries")?;
        }
        Field::FdStoreMax => {
            builder.fd_store_max = decode_dword_field(value, "FdStoreMax")?;
        }
        Field::Environment => {
            builder.environment =
                parse_environment_variables(decode_multi_sz_field(value, "Environment")?)?;
        }
        Field::WorkingDirectory => {
            builder.working_directory = parse_absolute_path_field(value, Field::WorkingDirectory)?;
        }
        Field::LimitNoFile => {
            builder.limit_nofile = Some(u64::from(decode_dword_field(value, "LimitNOFILE")?));
        }
        Field::LimitCore => {
            builder.limit_core = Some(u64::from(decode_dword_field(value, "LimitCORE")?));
        }
        Field::Conditions => {
            builder.conditions = parse_service_checks(value, Field::Conditions)?;
        }
        Field::Asserts => {
            builder.asserts = parse_service_checks(value, Field::Asserts)?;
        }
        Field::DisplayName => {
            builder.display_name = parse_optional_string(value, Field::DisplayName)?;
        }
        Field::Description => {
            builder.description = parse_optional_string(value, Field::Description)?;
        }
        Field::RestartPolicy => {
            builder.restart_policy =
                parse_restart_policy(decode_dword_field(value, "RestartPolicy")?)?;
        }
        Field::RestartMaxRetries => {
            builder.restart_max_retries = decode_dword_field(value, "RestartMaxRetries")?;
        }
        Field::RestartWindow => {
            builder.restart_window_secs = u64::from(decode_dword_field(value, "RestartWindow")?);
        }
        Field::RestartDelay => {
            builder.restart_delay_secs = u64::from(decode_dword_field(value, "RestartDelay")?);
        }
        Field::ErrorControl => {
            builder.error_control =
                parse_error_control(decode_dword_field(value, "ErrorControl")?)?;
        }
        Field::ServiceSecurity => {
            builder.service_security = ServiceSecurityDescriptor::RegistryBinary(
                decode_binary_field(value, "ServiceSecurity")?,
            );
        }
        Field::TimerPersistent => {
            builder.timer_persistent =
                parse_timer_persistent(decode_dword_field(value, "TimerPersistent")?)?;
        }
        Field::TimerJitter => {
            builder.timer_jitter_secs = u64::from(decode_dword_field(value, "TimerJitter")?);
        }
    }
    Ok(())
}
