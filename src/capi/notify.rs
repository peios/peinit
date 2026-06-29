use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixDatagram;

use libc::{c_char, c_int, c_ulonglong};

use crate::notify::parse_notify_message;

use super::error::{ErrorDetail, c_str_arg, ffi_status, utf8_arg};
use super::types::peinit_error_t;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_notify_send(
    message: *const c_char,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        let socket = std::env::var_os("NOTIFY_SOCKET")
            .ok_or_else(|| ErrorDetail::unavailable("NOTIFY_SOCKET is not set"))?;
        send_notify_payload(socket.as_os_str(), message)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_notify_send_to(
    socket_path: *const c_char,
    message: *const c_char,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        let socket_path = c_str_arg(socket_path, "socket_path")?;
        let socket_path = OsStr::from_bytes(socket_path.to_bytes());
        send_notify_payload(socket_path, message)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_notify_ready(error_out: *mut *mut peinit_error_t) -> c_int {
    notify_static(c"READY=1".as_ptr(), error_out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_notify_reloading(error_out: *mut *mut peinit_error_t) -> c_int {
    notify_static(c"RELOADING=1".as_ptr(), error_out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_notify_stopping(error_out: *mut *mut peinit_error_t) -> c_int {
    notify_static(c"STOPPING=1".as_ptr(), error_out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_notify_watchdog(error_out: *mut *mut peinit_error_t) -> c_int {
    notify_static(c"WATCHDOG=1".as_ptr(), error_out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_notify_status(
    status: *const c_char,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        let status = utf8_arg(status, "status")?;
        if status.contains('\n') || status.contains('\r') {
            return Err(ErrorDetail::invalid_argument(
                "status must not contain a newline",
            ));
        }
        let message = format!("STATUS={status}");
        send_notify_message_from_env(&message)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_notify_watchdog_usec(
    usec: c_ulonglong,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        let message = format!("WATCHDOG_USEC={usec}");
        send_notify_message_from_env(&message)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_notify_extend_timeout_usec(
    usec: c_ulonglong,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        let message = format!("EXTEND_TIMEOUT_USEC={usec}");
        send_notify_message_from_env(&message)
    })
}

fn notify_static(message: *const c_char, error_out: *mut *mut peinit_error_t) -> c_int {
    ffi_status(error_out, || {
        let message = utf8_arg(message, "message")?;
        send_notify_message_from_env(message)
    })
}

fn send_notify_payload(socket_path: &OsStr, message: *const c_char) -> Result<(), ErrorDetail> {
    let message = utf8_arg(message, "message")?;
    validate_notify_message(message)?;
    send_datagram(socket_path, message.as_bytes())
}

fn send_notify_message_from_env(message: &str) -> Result<(), ErrorDetail> {
    validate_notify_message(message)?;
    let socket = std::env::var_os("NOTIFY_SOCKET")
        .ok_or_else(|| ErrorDetail::unavailable("NOTIFY_SOCKET is not set"))?;
    send_datagram(socket.as_os_str(), message.as_bytes())
}

fn validate_notify_message(message: &str) -> Result<(), ErrorDetail> {
    if message.is_empty() {
        return Err(ErrorDetail::invalid_argument("message is empty"));
    }
    parse_notify_message(message.as_bytes())
        .map(|_| ())
        .map_err(|error| {
            ErrorDetail::invalid_argument(format!("invalid notify message: {error:?}"))
        })
}

fn send_datagram(socket_path: &OsStr, payload: &[u8]) -> Result<(), ErrorDetail> {
    if socket_path.is_empty() {
        return Err(ErrorDetail::invalid_argument("notify socket path is empty"));
    }
    let socket = UnixDatagram::unbound()
        .map_err(|error| ErrorDetail::io(format!("notify socket: {error}")))?;
    socket
        .connect(socket_path)
        .map_err(|error| ErrorDetail::io(format!("connect notify socket: {error}")))?;
    socket
        .send(payload)
        .map(|_| ())
        .map_err(|error| ErrorDetail::io(format!("send notify datagram: {error}")))
}
