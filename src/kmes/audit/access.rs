use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::control::service_security::ServiceAccessDenied;
use crate::control::system::SystemAccessDenied;

use crate::kmes::payload::{finish_event, write_str_field, write_uint_field};

pub fn encode_system_access_denied_event(
    denied: &SystemAccessDenied,
) -> Result<KmesEvent, BoundaryError> {
    encode_access_denied_event(
        denied.caller.caller_sid(),
        "system",
        "peinit_control",
        system_access_label(denied.desired_access.bits()),
        denied.desired_access.bits(),
        denied.granted_access_bits,
    )
}

pub fn encode_service_access_denied_event(
    denied: &ServiceAccessDenied,
) -> Result<KmesEvent, BoundaryError> {
    encode_access_denied_event(
        denied.caller.caller_sid(),
        "service",
        &denied.service,
        service_access_label(denied.desired_access.bits()),
        denied.desired_access.bits(),
        denied.granted_access_bits,
    )
}

fn encode_access_denied_event(
    caller_sid: &str,
    target_type: &str,
    target: &str,
    requested_right: &str,
    requested_access_bits: u32,
    granted_access_bits: u32,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(6);
    write_str_field(&mut writer, "caller_sid", caller_sid);
    write_str_field(&mut writer, "target_type", target_type);
    write_str_field(&mut writer, "target", target);
    write_str_field(&mut writer, "requested_right", requested_right);
    write_uint_field(
        &mut writer,
        "requested_access_bits",
        u64::from(requested_access_bits),
    );
    write_uint_field(
        &mut writer,
        "granted_access_bits",
        u64::from(granted_access_bits),
    );
    finish_event("access.denied", writer)
}

fn system_access_label(bits: u32) -> &'static str {
    match bits {
        0x0001 => "SYSTEM_SHUTDOWN",
        0x0002 => "SYSTEM_RELOAD_CONFIG",
        _ => "SYSTEM_ACCESS",
    }
}

fn service_access_label(bits: u32) -> &'static str {
    match bits {
        0x0001 => "SERVICE_QUERY_STATUS",
        0x0002 => "SERVICE_START",
        0x0004 => "SERVICE_STOP",
        0x0008 => "SERVICE_INTERROGATE",
        0x0006 => "SERVICE_START|SERVICE_STOP",
        0x000f => "SERVICE_ALL",
        _ => "SERVICE_ACCESS",
    }
}
