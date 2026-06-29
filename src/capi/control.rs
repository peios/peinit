use std::ffi::CString;

use libc::{c_char, c_int};
use serde_json::{Value, json};

use crate::control::client::{ControlClient, ControlClientError, ControlResponse};
use crate::control::socket::CONTROL_SOCKET_PATH;

use super::error::{ErrorDetail, ffi_status, utf8_arg};
use super::types::{
    PEINIT_SHUTDOWN_HALT, PEINIT_SHUTDOWN_POWEROFF, PEINIT_SHUTDOWN_REBOOT, peinit_client_t,
    peinit_error_t, peinit_response_t,
};

struct ResponseObject {
    raw_json: CString,
    status: CString,
    error_code: Option<CString>,
    error_message: Option<CString>,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_client_connect_default(
    out: *mut *mut peinit_client_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    unsafe { peinit_client_connect_path(c"/run/peinit/control.sock".as_ptr(), out, error_out) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_client_connect_path(
    path: *const c_char,
    out: *mut *mut peinit_client_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        clear_client_out(out)?;
        let path = utf8_arg(path, "path")?;
        let client = ControlClient::connect_path(path).map_err(ErrorDetail::from)?;
        unsafe {
            *out = Box::into_raw(Box::new(client)).cast::<peinit_client_t>();
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_client_free(client: *mut peinit_client_t) {
    if !client.is_null() {
        unsafe {
            drop(Box::from_raw(client.cast::<ControlClient>()));
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_control_raw_json(
    client: *mut peinit_client_t,
    request_json: *const c_char,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        clear_response_out(response_out)?;
        let request_json = utf8_arg(request_json, "request_json")?;
        if request_json.as_bytes().contains(&b'\n') || request_json.as_bytes().contains(&b'\r') {
            return Err(ErrorDetail::invalid_argument(
                "request_json must contain exactly one JSON object",
            ));
        }
        let request: Value = serde_json::from_str(request_json)
            .map_err(|_| ErrorDetail::invalid_argument("request_json is not valid JSON"))?;
        send_request(client, request, response_out)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_service_start(
    client: *mut peinit_client_t,
    service: *const c_char,
    wait: bool,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    service_command(client, "start", service, wait, response_out, error_out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_service_stop(
    client: *mut peinit_client_t,
    service: *const c_char,
    wait: bool,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    service_command(client, "stop", service, wait, response_out, error_out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_service_restart(
    client: *mut peinit_client_t,
    service: *const c_char,
    wait: bool,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    service_command(client, "restart", service, wait, response_out, error_out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_service_reload(
    client: *mut peinit_client_t,
    service: *const c_char,
    wait: bool,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    service_command(client, "reload", service, wait, response_out, error_out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_service_reset(
    client: *mut peinit_client_t,
    service: *const c_char,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    service_command(client, "reset", service, false, response_out, error_out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_service_status(
    client: *mut peinit_client_t,
    service: *const c_char,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    service_command(client, "status", service, false, response_out, error_out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_service_list(
    client: *mut peinit_client_t,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        clear_response_out(response_out)?;
        send_request(client, json!({ "command": "list" }), response_out)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_operation_status(
    client: *mut peinit_client_t,
    operation_id: *const c_char,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        clear_response_out(response_out)?;
        let operation_id = utf8_arg(operation_id, "operation_id")?;
        send_request(
            client,
            json!({
                "command": "operation-status",
                "operation_id": operation_id,
            }),
            response_out,
        )
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_reload_config(
    client: *mut peinit_client_t,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        clear_response_out(response_out)?;
        send_request(client, json!({ "command": "reload-config" }), response_out)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_system_shutdown(
    client: *mut peinit_client_t,
    shutdown_kind: c_int,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        clear_response_out(response_out)?;
        let shutdown_type = match shutdown_kind {
            PEINIT_SHUTDOWN_POWEROFF => "poweroff",
            PEINIT_SHUTDOWN_REBOOT => "reboot",
            PEINIT_SHUTDOWN_HALT => "halt",
            _ => return Err(ErrorDetail::invalid_argument("invalid shutdown_kind")),
        };
        send_request(
            client,
            json!({
                "command": "shutdown",
                "type": shutdown_type,
            }),
            response_out,
        )
    })
}

fn service_command(
    client: *mut peinit_client_t,
    command: &'static str,
    service: *const c_char,
    wait: bool,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        clear_response_out(response_out)?;
        let service = utf8_arg(service, "service")?;
        send_request(
            client,
            json!({
                "command": command,
                "service": service,
                "wait": wait,
            }),
            response_out,
        )
    })
}

fn send_request(
    client: *mut peinit_client_t,
    request: Value,
    response_out: *mut *mut peinit_response_t,
) -> Result<(), ErrorDetail> {
    let client = client_mut(client)?;
    let response = client.request(request).map_err(ErrorDetail::from)?;
    let response = response_from_control(response)?;
    unsafe {
        *response_out = Box::into_raw(Box::new(response)).cast::<peinit_response_t>();
    }
    Ok(())
}

fn response_from_control(response: ControlResponse) -> Result<ResponseObject, ErrorDetail> {
    let raw_json = CString::new(response.raw_json())
        .map_err(|_| ErrorDetail::protocol("control response contains an interior NUL byte"))?;
    let status = CString::new(response.status().as_str())
        .map_err(|_| ErrorDetail::protocol("invalid response status"))?;
    let error_code = response
        .error_code()
        .map(CString::new)
        .transpose()
        .map_err(|_| ErrorDetail::protocol("invalid response error code"))?;
    let error_message = response
        .error_message()
        .map(CString::new)
        .transpose()
        .map_err(|_| ErrorDetail::protocol("invalid response error message"))?;

    Ok(ResponseObject {
        raw_json,
        status,
        error_code,
        error_message,
    })
}

fn clear_client_out(out: *mut *mut peinit_client_t) -> Result<(), ErrorDetail> {
    if out.is_null() {
        return Err(ErrorDetail::invalid_argument(
            "client out parameter is NULL",
        ));
    }
    unsafe {
        *out = std::ptr::null_mut();
    }
    Ok(())
}

fn clear_response_out(out: *mut *mut peinit_response_t) -> Result<(), ErrorDetail> {
    if out.is_null() {
        return Err(ErrorDetail::invalid_argument(
            "response out parameter is NULL",
        ));
    }
    unsafe {
        *out = std::ptr::null_mut();
    }
    Ok(())
}

fn client_mut<'a>(client: *mut peinit_client_t) -> Result<&'a mut ControlClient, ErrorDetail> {
    if client.is_null() {
        return Err(ErrorDetail::invalid_argument("client is NULL"));
    }
    Ok(unsafe { &mut *client.cast::<ControlClient>() })
}

fn response_ref<'a>(response: *const peinit_response_t) -> Result<&'a ResponseObject, ErrorDetail> {
    if response.is_null() {
        return Err(ErrorDetail::invalid_argument("response is NULL"));
    }
    Ok(unsafe { &*response.cast::<ResponseObject>() })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_response_free(response: *mut peinit_response_t) {
    if !response.is_null() {
        unsafe {
            drop(Box::from_raw(response.cast::<ResponseObject>()));
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_response_json(response: *const peinit_response_t) -> *const c_char {
    match response_ref(response) {
        Ok(response) => response.raw_json.as_ptr(),
        Err(_) => std::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_response_status(
    response: *const peinit_response_t,
) -> *const c_char {
    match response_ref(response) {
        Ok(response) => response.status.as_ptr(),
        Err(_) => std::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_response_is_ok(response: *const peinit_response_t) -> c_int {
    match response_ref(response) {
        Ok(response) if response.status.as_bytes() == b"ok" => 1,
        Ok(_) => 0,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_response_error_code(
    response: *const peinit_response_t,
) -> *const c_char {
    match response_ref(response) {
        Ok(response) => response
            .error_code
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr()),
        Err(_) => std::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_response_error_message(
    response: *const peinit_response_t,
) -> *const c_char {
    match response_ref(response) {
        Ok(response) => response
            .error_message
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr()),
        Err(_) => std::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn peinit_default_control_socket_path() -> *const c_char {
    c"/run/peinit/control.sock".as_ptr()
}

#[allow(dead_code)]
fn _assert_default_path_matches_runtime() {
    assert_eq!(CONTROL_SOCKET_PATH, "/run/peinit/control.sock");
}

impl From<ControlClientError> for ErrorDetail {
    fn from(error: ControlClientError) -> Self {
        match error {
            ControlClientError::Io(message) => Self::io(message),
            ControlClientError::Protocol(message) => Self::protocol(message),
        }
    }
}
