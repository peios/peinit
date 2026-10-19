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

/// Clear the caller-owned error slot and contain unwinding at the C boundary.
/// The slot follows the module output-pointer contract.
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

/// Borrow a caller-owned NUL-terminated string under the C entry-point contract.
/// The inferred lifetime must not outlive the caller's input allocation.
pub(super) fn c_str_arg<'a>(ptr: *const c_char, name: &str) -> Result<&'a CStr, ErrorDetail> {
    if ptr.is_null() {
        return Err(ErrorDetail::invalid_argument(format!("{name} is NULL")));
    }
    // SAFETY: C entry points require a live NUL-terminated input for this borrow.
    Ok(unsafe { CStr::from_ptr(ptr) })
}

pub(super) fn utf8_arg<'a>(ptr: *const c_char, name: &str) -> Result<&'a str, ErrorDetail> {
    c_str_arg(ptr, name)?
        .to_str()
        .map_err(|_| ErrorDetail::invalid_argument(format!("{name} is not valid UTF-8")))
}

fn clear_error(error_out: *mut *mut peinit_error_t) {
    if !error_out.is_null() {
        // SAFETY: the entry-point contract grants unique writable access to this slot.
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
        // SAFETY: the entry-point contract grants unique writable access to this slot.
        unsafe {
            *error_out = Box::into_raw(object).cast::<peinit_error_t>();
        }
    }
    code
}

/// # Safety
/// Non-null handles must be live library objects with exclusive access.
/// A non-null argument is consumed and must not be used or freed again.
/// The module pointer contracts and the matching public header define null handling
/// and the lifetime of borrowed results.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_error_free(error: *mut peinit_error_t) {
    if !error.is_null() {
        // SAFETY: the caller transfers this live library allocation back exactly once.
        unsafe {
            drop(Box::from_raw(error.cast::<ErrorObject>()));
        }
    }
}

/// # Safety
/// Handles must be live library objects; mutating calls require exclusive access.
/// The module pointer contracts and the matching public header define null handling
/// and the lifetime of borrowed results.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_error_code(error: *const peinit_error_t) -> c_int {
    if error.is_null() {
        return 0;
    }
    // SAFETY: the caller keeps the library error alive for this read.
    unsafe { (*error.cast::<ErrorObject>()).code }
}

/// # Safety
/// Handles must be live library objects; mutating calls require exclusive access.
/// The module pointer contracts and the matching public header define null handling
/// and the lifetime of borrowed results.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_error_message(error: *const peinit_error_t) -> *const c_char {
    if error.is_null() {
        return std::ptr::null();
    }
    // SAFETY: the caller keeps the error alive for this access and the returned borrow.
    unsafe { (*error.cast::<ErrorObject>()).message.as_ptr() }
}

/// # Safety
/// A non-null argument is consumed and must not be used or freed again.
/// The module pointer contracts and the matching public header define null handling
/// and the lifetime of borrowed results.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_string_free(value: *mut c_char) {
    if !value.is_null() {
        // SAFETY: the contract accepts only an owned CString returned by this library.
        unsafe {
            drop(CString::from_raw(value));
        }
    }
}
