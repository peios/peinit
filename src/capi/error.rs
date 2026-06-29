use std::ffi::{CStr, CString};
use std::panic::{AssertUnwindSafe, catch_unwind};

use libc::{c_char, c_int};

use super::types::{
    PEINIT_ERR_INTERNAL, PEINIT_ERR_INVALID_ARGUMENT, PEINIT_ERR_IO, PEINIT_ERR_PROTOCOL,
    PEINIT_ERR_UNAVAILABLE, peinit_error_t,
};

pub(super) struct ErrorDetail {
    pub(super) code: c_int,
    pub(super) message: String,
}

struct ErrorObject {
    code: c_int,
    message: CString,
}

impl ErrorDetail {
    pub(super) fn invalid_argument(message: impl Into<String>) -> Self {
        Self {
            code: PEINIT_ERR_INVALID_ARGUMENT,
            message: message.into(),
        }
    }

    pub(super) fn io(message: impl Into<String>) -> Self {
        Self {
            code: PEINIT_ERR_IO,
            message: message.into(),
        }
    }

    pub(super) fn protocol(message: impl Into<String>) -> Self {
        Self {
            code: PEINIT_ERR_PROTOCOL,
            message: message.into(),
        }
    }

    pub(super) fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: PEINIT_ERR_UNAVAILABLE,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: PEINIT_ERR_INTERNAL,
            message: message.into(),
        }
    }
}

pub(super) fn ffi_status<F>(error_out: *mut *mut peinit_error_t, f: F) -> c_int
where
    F: FnOnce() -> Result<(), ErrorDetail>,
{
    clear_error(error_out);
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => write_error(error_out, error),
        Err(_) => write_error(
            error_out,
            ErrorDetail::internal("panic crossed libpeinit C ABI boundary"),
        ),
    }
}

pub(super) fn c_str_arg<'a>(ptr: *const c_char, name: &str) -> Result<&'a CStr, ErrorDetail> {
    if ptr.is_null() {
        return Err(ErrorDetail::invalid_argument(format!("{name} is NULL")));
    }
    Ok(unsafe { CStr::from_ptr(ptr) })
}

pub(super) fn utf8_arg<'a>(ptr: *const c_char, name: &str) -> Result<&'a str, ErrorDetail> {
    c_str_arg(ptr, name)?
        .to_str()
        .map_err(|_| ErrorDetail::invalid_argument(format!("{name} is not valid UTF-8")))
}

fn clear_error(error_out: *mut *mut peinit_error_t) {
    if !error_out.is_null() {
        unsafe {
            *error_out = std::ptr::null_mut();
        }
    }
}

fn write_error(error_out: *mut *mut peinit_error_t, error: ErrorDetail) -> c_int {
    let code = error.code;
    if !error_out.is_null() {
        let message = match CString::new(error.message) {
            Ok(message) => message,
            Err(error) => {
                let mut bytes = error.into_vec();
                for byte in &mut bytes {
                    if *byte == 0 {
                        *byte = b'?';
                    }
                }
                CString::new(bytes)
                    .unwrap_or_else(|_| CString::new("libpeinit error").expect("static string"))
            }
        };
        let object = Box::new(ErrorObject { code, message });
        unsafe {
            *error_out = Box::into_raw(object).cast::<peinit_error_t>();
        }
    }
    code
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_error_free(error: *mut peinit_error_t) {
    if !error.is_null() {
        unsafe {
            drop(Box::from_raw(error.cast::<ErrorObject>()));
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_error_code(error: *const peinit_error_t) -> c_int {
    if error.is_null() {
        return 0;
    }
    unsafe { (*error.cast::<ErrorObject>()).code }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_error_message(error: *const peinit_error_t) -> *const c_char {
    if error.is_null() {
        return std::ptr::null();
    }
    unsafe { (*error.cast::<ErrorObject>()).message.as_ptr() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_string_free(value: *mut c_char) {
    if !value.is_null() {
        unsafe {
            drop(CString::from_raw(value));
        }
    }
}
