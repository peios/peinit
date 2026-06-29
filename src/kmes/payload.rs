use peios::msgpack::{self, Writer};

use crate::boundary::{BoundaryError, KmesEvent};
use crate::security::TokenSummary;

pub(super) fn finish_event(
    event_type: &'static str,
    writer: Writer,
) -> Result<KmesEvent, BoundaryError> {
    let payload = writer
        .to_bytes()
        .map_err(|error| BoundaryError::Kmes(error.to_string()))?;
    msgpack::validate(&payload, msgpack::DEFAULT_MAX_DEPTH)
        .map_err(|error| BoundaryError::Kmes(error.to_string()))?;
    Ok(KmesEvent::new(event_type, payload))
}

pub(super) fn write_str_field(writer: &mut Writer, key: &str, value: &str) {
    writer.write_str(key).write_str(value);
}

pub(super) fn write_optional_str_field(writer: &mut Writer, key: &str, value: Option<&str>) {
    writer.write_str(key);
    match value {
        Some(value) => {
            writer.write_str(value);
        }
        None => {
            writer.write_nil();
        }
    }
}

pub(super) fn write_optional_string_field(writer: &mut Writer, key: &str, value: Option<String>) {
    writer.write_str(key);
    match value {
        Some(value) => {
            writer.write_str(&value);
        }
        None => {
            writer.write_nil();
        }
    }
}

pub(super) fn write_uint_field(writer: &mut Writer, key: &str, value: u64) {
    writer.write_str(key).write_uint(value);
}

pub(super) fn write_optional_u64_field(writer: &mut Writer, key: &str, value: Option<u64>) {
    writer.write_str(key);
    match value {
        Some(value) => {
            writer.write_uint(value);
        }
        None => {
            writer.write_nil();
        }
    }
}

pub(super) fn write_optional_u32_field(writer: &mut Writer, key: &str, value: Option<u32>) {
    write_optional_u64_field(writer, key, value.map(u64::from));
}

pub(super) fn write_optional_i32_field(writer: &mut Writer, key: &str, value: Option<i32>) {
    writer.write_str(key);
    match value {
        Some(value) => {
            writer.write_int(i64::from(value));
        }
        None => {
            writer.write_nil();
        }
    }
}

pub(super) fn write_string_array_field(writer: &mut Writer, key: &str, values: &[String]) {
    writer
        .write_str(key)
        .write_array(u32::try_from(values.len()).unwrap_or(u32::MAX));
    for value in values {
        writer.write_str(value);
    }
}

pub(super) fn write_token_summary_field(writer: &mut Writer, key: &str, token: &TokenSummary) {
    writer.write_str(key);
    write_token_summary(writer, token);
}

pub(super) fn write_optional_token_summary_field(
    writer: &mut Writer,
    key: &str,
    token: Option<&TokenSummary>,
) {
    writer.write_str(key);
    match token {
        Some(token) => {
            write_token_summary(writer, token);
        }
        None => {
            writer.write_nil();
        }
    }
}

fn write_token_summary(writer: &mut Writer, token: &TokenSummary) {
    writer.write_map(5);
    write_str_field(writer, "identity", &token.identity);
    write_str_field(writer, "user_sid", &token.user_sid);
    write_string_array_field(writer, "group_sids", &token.group_sids);
    write_string_array_field(writer, "present_privileges", &token.present_privileges);
    write_string_array_field(writer, "enabled_privileges", &token.enabled_privileges);
}
